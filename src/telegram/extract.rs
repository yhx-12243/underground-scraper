use core::pin::pin;
use std::{
    cell::RefCell,
    fs::File,
    io::{BufWriter, Write},
};

use compact_str::CompactString;
use futures_util::TryStreamExt;
use hashbrown::{
    hash_set::{Entry, HashSet},
    HashMap,
};
use tokio_postgres::Client;
use uscr::{
    db::{get_connection, BB8Error, DBResult, ToSqlIter},
    util::xmax_to_success,
};

const fn idc(ch: u8) -> bool {
    matches!(ch, b'0'..=b'9' | b'A' ..= b'Z' | b'a' ..= b'z' | b'-' | b'_')
}

struct Saver {
    dict: HashSet<CompactString>,
    sf: BufWriter<File>,
}

pub struct Inspector {
    dict: HashSet<CompactString>,
    es: Vec<(i64, i32, CompactString)>,
    map: HashMap<CompactString, i64>,

    saver: RefCell<Saver>,
}

impl Inspector {
    pub fn new(map: HashMap<CompactString, i64>, file: File) -> Self {
        Self {
            dict: HashSet::new(),
            es: Vec::new(),
            map,
            saver: RefCell::new(Saver {
                dict: HashSet::new(),
                sf: BufWriter::new(file),
            }),
        }
    }

    fn inspect(&mut self, channel_id: i64, message_id: i32, text: &str) {
        'fo: for (idx, _) in text.match_indices('t') {
            let suffix = 'inspect: {
                // SAFETY: 0 <= idx < text.len().
                let mut suffix = unsafe { text.get_unchecked(idx + 1..) };
                if let Some(s) = suffix.strip_prefix("elegram") {
                    suffix = s;
                } else if let Some(s) = suffix.strip_prefix("elesco") {
                    suffix = s;
                } else if let Some(s) = suffix.strip_prefix("g://join") {
                    break 'inspect s;
                } else if let Some(s) = suffix.strip_prefix('g') {
                    suffix = s;
                }
                if let Some(s) = suffix.strip_prefix(".me/") {
                    suffix = s;
                } else if let Some(s) = suffix.strip_prefix(".dev/") {
                    suffix = s;
                } else if let Some(s) = suffix.strip_prefix(".dog/") {
                    suffix = s;
                } else if let Some(s) = suffix.strip_prefix(".pe/") {
                    suffix = s;
                } else {
                    continue 'fo;
                }
                suffix.strip_prefix("joinchat").unwrap_or(suffix)
            };
            let suffix = suffix.as_bytes();
            let Some(i) = suffix[..8.min(suffix.len())].iter().position(|ch| idc(*ch)) else {
                continue;
            };
            // SAFETY: 0 <= i < suffix.len().
            let suffix = unsafe { suffix.get_unchecked(i..) };
            let len = suffix
                .iter()
                .position(|ch| !idc(*ch))
                .unwrap_or(suffix.len());
            let mut result =
                // SAFETY: suffix[len] is ASCII, which is UTF-8 boundary.
                unsafe { CompactString::from_utf8_unchecked(suffix.get_unchecked(..len)) };

            match self.dict.entry(result) {
                Entry::Occupied(e) => result = e.into_key().unwrap(),
                Entry::Vacant(e) => {
                    let prefix =
                        // SAFETY: the position of suffix is UTF-8 boundary.
                        unsafe { text.get_unchecked(idx..suffix.as_ptr().sub_ptr(text.as_ptr())) };
                    tracing::info!(target: "telegram-extractor", "found: https://\x1b[33m{}\x1b[36m{}\x1b[0m", prefix, e.get());
                    result = e.get().clone();
                    e.insert();
                }
            }

            self.es.push((channel_id, message_id, result));
        }
    }

    async fn commit(&self, data: &[(i64, i32, CompactString)], conn: &mut Client) -> DBResult<()> {
        const SQL: &str = "with tmp_insert(c1, m, c2) as (select * from unnest($1::bigint[], $2::integer[], $3::bigint[])) insert into telegram.link (c1, message_id, c2) select c1, m, c2 from tmp_insert on conflict (c1, message_id, c2) do nothing returning xmax";

        if data.is_empty() {
            return Ok(());
        }

        let mut batch = Vec::with_capacity(data.len());
        {
            let mut saver = self.saver.borrow_mut();
            for (channel_id, message_id, result) in data {
                if let Some(id2) = self.map.get(result) {
                    if channel_id != id2 {
                        batch.push((channel_id, message_id, id2));
                    }
                } else if saver.dict.insert(result.clone()) {
                    let _ = saver.sf.write_all(result.as_bytes());
                    let _ = saver.sf.write_all(b"\n");
                }
            }
        }

        let stmt = conn.prepare_static(SQL.into()).await?;
        let rows = conn
            .query(
                &stmt,
                &[
                    &ToSqlIter(batch.iter().map(|x| x.0)),
                    &ToSqlIter(batch.iter().map(|x| x.1)),
                    &ToSqlIter(batch.iter().map(|x| x.2)),
                ],
            )
            .await?;

        tracing::info!(target: "telegram-committer", "\x1b[32m{}\x1b[0m/\x1b[33m{}\x1b[0m links added.", xmax_to_success(rows.iter()), batch.len());

        Ok(())
    }

    pub async fn extract_content(
        &mut self,
        id: i64,
        limit: Option<u32>,
    ) -> Result<(), uscr::db::BB8Error> {
        const SQL: &str = "select channel_id, message_id, data->>'message' from telegram.message where channel_id = $1 and data->>'message' like '%t%'";
        const SQL_WITH_LIMIT: &str = "select channel_id, message_id, data->>'message' from telegram.message where channel_id = $1 and data->>'message' like '%t%' order by message_id desc limit $2";

        let mut conn = get_connection().await?;
        let stream = if let Some(limit) = limit {
            let stmt = conn.prepare_static(SQL_WITH_LIMIT.into()).await?;
            conn.query_raw(&stmt, &[&id, &(limit as i64)]).await
        } else {
            let stmt = conn.prepare_static(SQL.into()).await?;
            conn.query_raw(&stmt, &[&id]).await
        }?;
        let mut stream = pin!(stream);

        let last_len = self.es.len();
        while let Some(row) = stream.try_next().await? {
            if let Ok(channel_id) = row.try_get(0)
                && let Ok(message_id) = row.try_get(1)
                && let Ok(content) = row.try_get(2)
            {
                self.inspect(channel_id, message_id, content);
            }
        }

        let n = self.es.len() - last_len;
        if n != 0 {
            tracing::info!(target: "telegram-extractor", "[channel \x1b[36m{id}\x1b[0m] \x1b[33m{n}\x1b[0m data collected");
        }

        self.commit(&self.es[last_len..], &mut conn)
            .await
            .map_err(Into::into)
    }
}

pub async fn generate_user_id_map() -> Result<HashMap<CompactString, i64>, BB8Error> {
    const SQL1: &str = "select channel_id, hash from telegram.invite";
    const SQL2: &str = "select id, name from telegram.channel";

    let mut conn = get_connection().await?;
    let stmt = conn.prepare_static(SQL1.into()).await?;
    let rows = conn.query(&stmt, &[]).await?;
    let iter1 = rows.into_iter();

    let stmt = conn.prepare_static(SQL2.into()).await?;
    let rows = conn.query(&stmt, &[]).await?;
    let iter2 = rows.into_iter();

    Ok(iter1
        .chain(iter2)
        .filter_map(|row| {
            let id = row.try_get(0).ok()?;
            let name = row.try_get::<_, &str>(1).ok()?;
            Some((name.into(), id))
        })
        .collect())
}
