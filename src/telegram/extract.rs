use core::pin::pin;

use compact_str::CompactString;
use futures_util::TryStreamExt;
use hashbrown::hash_set::{Entry, HashSet};
use uscr::db::get_connection;

const fn idc(ch: u8) -> bool {
    matches!(ch, b'0'..=b'9' | b'A' ..= b'Z' | b'a' ..= b'z' | b'-' | b'_')
}

#[derive(Default)]
struct Inspector {
    dict: HashSet<CompactString>,
    es: Vec<(i64, i64, CompactString)>,
}

impl Inspector {
    fn inspect(&mut self, channel_id: i64,  message_id:i64,text: &str) {
        'fo: for (idx, _) in text.match_indices('t') {
            let suffix = 'inspect: {
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
            let suffix = unsafe { suffix.get_unchecked(i..) };
            let len = suffix
                .iter()
                .position(|ch| !idc(*ch))
                .unwrap_or(suffix.len());
            let mut result = unsafe { CompactString::from_utf8_unchecked(suffix.get_unchecked(..len)) };

            match self.dict.entry(result) {
                Entry::Occupied(e) => {
                    result = e.into_key().unwrap();
                },
                Entry::Vacant(e) => { 
                    let prefix = unsafe { text.get_unchecked(idx..suffix.as_ptr().sub_ptr(text.as_ptr())) };
                    tracing::info!(target: "telegram-extractor", "found: https://\x1b[33m{}\x1b[36m{}\x1b[0m", prefix, e.get());
                    result = e.get().clone();
                    e.insert();
                }
            }

            self.es.push((channel_id, message_id, result));
        }
    }
}

pub async fn extract_content(id: i64, limit: u32) -> Result<(), uscr::db::BB8Error> {
    const SQL: &str =
        "select channel_id, message_id, data->>'message' from telegram.message where channel_id = $1 and data->>'message' like '%t%' order by message_id desc limit $2";

    let mut conn = get_connection().await?;
    let stmt = conn.prepare_static(SQL.into()).await?;
    let stream = conn.query_raw(&stmt, &[&id, &(limit as i64)]).await?;
    let mut stream = pin!(stream);

    let mut inspector = Inspector::default();

    while let Some(row) = stream.try_next().await? {
        if let Ok(channel_id) = row.try_get(0)
            && let Ok(message_id) = row.try_get(1)
            && let Ok(content) = row.try_get(2)
        {
            inspector.inspect(channel_id, message_id, content);
        }
    }

    Ok(())
}
