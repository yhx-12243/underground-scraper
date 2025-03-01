use std::{
    fs::File,
    io::{BufWriter, IoSlice, Write},
    pin::pin,
};

use compact_str::CompactString;
use futures_util::TryStreamExt;
use hashbrown::{HashMap, hash_map::RawEntryMut, hash_set::HashSet};
use tokio_postgres::{Client, types::ToSql};
use unicase::UniCase;
use uscr::{
    db::{DBError, DBResult, ToSqlIter, get_connection},
    util::{box_io_error, xmax_to_success},
};

const fn idc(ch: u8) -> bool {
    matches!(ch, b'0'..=b'9' | b'A' ..= b'Z' | b'a' ..= b'z' | b'-' | b'_')
}

pub struct Inspector {
    dict: HashSet<UniCase<CompactString>>,
    es: Vec<(i64, i32, UniCase<CompactString>)>,
    map: HashMap<UniCase<CompactString>, i64>,

    saver: BufWriter<File>,
}

impl Inspector {
    pub fn new(map: HashMap<UniCase<CompactString>, i64>, file: File) -> Self {
        Self {
            dict: HashSet::new(),
            es: Vec::new(),
            map,
            saver: BufWriter::new(file),
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
            let result = UniCase::new(
                // SAFETY: suffix[len] is ASCII, which is UTF-8 boundary.
                unsafe { CompactString::from_utf8_unchecked(suffix.get_unchecked(..len)) },
            );

            if let RawEntryMut::Vacant(e) = self.dict.raw_entry_mut().from_key(&result) {
                let prefix =
                    // SAFETY: the position of suffix is UTF-8 boundary.
                    unsafe { text.get_unchecked(idx..suffix.as_ptr().offset_from_unsigned(text.as_ptr())) };
                tracing::info!(target: "telegram-extractor", "found: https://\x1b[33m{prefix}\x1b[36m{result}\x1b[0m");
                e.insert(result.clone(), ());
            }

            self.es.push((channel_id, message_id, result));
        }
    }

    async fn commit(&mut self, conn: &mut Client) -> DBResult<()> {
        const SQL: &str = "with tmp_insert(c1, m, c2) as (select * from unnest($1::bigint[], $2::integer[], $3::bigint[])) insert into telegram.link (c1, message_id, c2) select c1, m, c2 from tmp_insert on conflict (c1, message_id, c2) do nothing returning xmax";

        let mut batch = Vec::with_capacity(self.es.len());
        for (channel_id, message_id, result) in core::mem::take(&mut self.es) {
            if let Some(&id2) = self.map.get(&result) && channel_id != id2 { // self reference
                batch.push((channel_id, message_id, id2));
            }
        }

        if batch.is_empty() {
            return Ok(());
        }

        let stmt = conn.prepare_static(SQL.into()).await?;
        let rows = conn
            .query(&stmt, &[
                &ToSqlIter(batch.iter().map(|x| x.0)),
                &ToSqlIter(batch.iter().map(|x| x.1)),
                &ToSqlIter(batch.iter().map(|x| x.2)),
            ])
            .await?;

        tracing::info!(target: "telegram-committer", "\x1b[32m{}\x1b[0m/\x1b[33m{}\x1b[0m links added.", xmax_to_success(rows.iter()), batch.len());

        Ok(())
    }

    pub async fn extract_content(&mut self, conn: &mut Client) -> Result<(), uscr::db::BB8Error> {
        const SQL: &str = "select channel_id, message_id, data->>'message' from telegram.message where data->>'message' like '%t%'";

        let mut conn_bg = get_connection().await?;
        let stmt = conn_bg.prepare_static(SQL.into()).await?;
        let stream = conn_bg.query_raw(&stmt, core::iter::empty::<&dyn ToSql>()).await?;
        let mut stream = pin!(stream);

        let mut cnt = 0;
        while let Some(row) = stream.try_next().await? {
            if let Ok(channel_id) = row.try_get(0) && let Ok(message_id) = row.try_get(1) && let Ok(content) = row.try_get(2) {
                self.inspect(channel_id, message_id, content);
            }
            cnt += 1;
            if cnt % 65536 == 0 {
                self.commit(conn).await?;
                self.es.clear();
            }
        }

        self.commit(conn).await?;

        for id in &self.dict {
            self.saver
                .write_all_vectored(&mut [IoSlice::new(id.as_bytes()), IoSlice::new(b"\n")])
                .map_err(|e| DBError::new(tokio_postgres::error::Kind::Io, Some(box_io_error(e))))?;
        }

        Ok(())
    }
}
