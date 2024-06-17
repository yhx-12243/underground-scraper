pub mod client;
mod types;

use std::{
    fs::File,
    future::join,
    io::{self, BufReader},
    path::Path,
};

use client::Client;
use compact_str::CompactString;
use grammers_client::client::bots::InvocationError;
use grammers_mtsender::RpcError;
use grammers_session::PackedChat;
use grammers_tl_types as tl;
use hashbrown::HashMap;
use tokio_postgres::types::Json;
use uscr::{
    db::{get_connection, BB8Error, DBResult, PooledConnection, ToSqlIter},
    util::xmax_to_success,
};

use types::Message;

pub fn parse_config(file: &Path) -> io::Result<HashMap<i32, String>> {
    let file = File::open(file)?;
    let reader = BufReader::new(file);
    serde_json::from_reader(reader).map_err(io::Error::other)
}

#[derive(Debug)]
pub struct Channel {
    pub id: i64,
    pub name: CompactString,
    pub access_hash: i64,
    pub app_id: i32,
}

pub async fn fetch_channels_by_id<C>(
    client: &Client,
    channels: C,
) -> Result<Vec<Channel>, InvocationError>
where
    C: Iterator<Item = i64>,
{
    use tl::{
        enums::{messages::Chats, Chat, InputChannel::Channel as Ch},
        types::{messages, InputChannel},
    };

    let request = tl::functions::channels::GetChannels {
        id: channels
            .map(|channel_id| {
                Ch(InputChannel {
                    channel_id,
                    access_hash: 0,
                })
            })
            .collect(),
    };

    let (Chats::Chats(messages::Chats { chats })
    | Chats::Slice(messages::ChatsSlice { chats, .. })) = client.inner.invoke(&request).await?;

    Ok(chats
        .into_iter()
        .filter_map(|chat| {
            let Chat::Channel(channel) = chat else {
                return None;
            };

            Some(Channel {
                id: channel.id,
                name: channel.username.unwrap_or(channel.title).into(),
                access_hash: channel.access_hash.unwrap_or(0),
                app_id: 0,
            })
        })
        .collect())
}

async fn insert_to_db(
    messages: &[(i32, Message)],
    channel_id: i64,
    interval: &mut Option<(i32, i32)>,
    target: &str,
) -> Option<i32> {
    async fn insert_to_db_inner(
        messages: &[(i32, Message)],
        channel_id: i64,
    ) -> Result<(usize, usize, i32, i32, PooledConnection), BB8Error> {
        const SQL: &str = "with tmp_insert(m, d) as (select * from unnest($1::integer[], $3::jsonb[])) insert into telegram.message (id, message_id, channel_id, data) select ($2::bigint << 32) | m, m, $2, d from tmp_insert on conflict (id) do update set message_id = excluded.message_id, channel_id = excluded.channel_id, data = excluded.data returning xmax";

        let len = messages.len();
        let min = messages.iter().fold(i32::MAX, |x, y| x.min(y.0));
        let max = messages.iter().fold(i32::MIN, |x, y| x.max(y.0));

        let mut conn = get_connection().await?;
        let stmt = conn.prepare_static(SQL.into()).await?;
        let rows = conn
            .query(
                &stmt,
                &[
                    &ToSqlIter(messages.iter().map(|x| x.0)),
                    &channel_id,
                    &ToSqlIter(messages.iter().map(|x| Json(&x.1))),
                ],
            )
            .await?;

        Ok((xmax_to_success(rows.iter()), len, min, max, conn))
    }

    if messages.is_empty() {
        log::warn!(target: target, "empty batch");
        None
    } else {
        match insert_to_db_inner(messages, channel_id).await {
            Ok((succ, len, min, max, mut conn)) => {
                log::info!(target: target, "{succ}/{len} data upserted, id range: [{min}, {max}]");
                let inner = match interval {
                    Some(inner) => {
                        inner.0 = inner.0.min(min);
                        inner.1 = inner.1.max(max);
                        inner
                    }
                    None => interval.insert((min, max)),
                };
                let e: DBResult<()> = try {
                    const SQL: &str = "update telegram.channel set min_message_id = $1, max_message_id = $2, last_fetch = now() at time zone 'UTC' where id = $3";
                    let stmt = conn.prepare_static(SQL.into()).await?;
                    conn.execute(&stmt, &[&inner.0, &inner.1, &channel_id])
                        .await?;
                };
                if let Err(e) = e {
                    log::error!(target: target, "{e:#?}");
                }
                Some(max)
            }
            Err(e) => {
                log::error!(target: target, "{e:#?}");
                None
            }
        }
    }
}

pub async fn fetch_content(client: &Client, channel: &Channel, limit: u32, target: &str) {
    log::info!(target: target, "======== \x1b[32mFETCHING CONTENT \x1b[36m{}\x1b[0m ========", channel.id);

    let packed = PackedChat {
        ty: grammers_session::PackedType::Broadcast,
        id: channel.id,
        access_hash: (channel.access_hash != 0).then_some(channel.access_hash),
    };

    // compute stop line
    let interval_origin: Option<(i32, i32)> = try {
        const SQL: &str =
            "select min_message_id, max_message_id from telegram.channel where id = $1";

        let mut conn = get_connection().await.ok()?;
        let stmt = conn.prepare_static(SQL.into()).await.ok()?;
        let row = conn.query_one(&stmt, &[&channel.id]).await.ok()?;

        let min: i32 = row.try_get(0).ok()?;
        let max: i32 = row.try_get(1).ok()?;

        if min == 0 && max == 0 {
            do yeet;
        } else {
            (min, max)
        }
    };
    let mut interval = interval_origin;
    let mut stop_point = interval.map_or(0, |x| x.1);

    let mut iter = client.inner.iter_messages(packed);
    let mut buffer = Vec::with_capacity(100);
    'outer: loop {
        let item = loop {
            let item = if let Some(raw) = iter.next_raw() {
                raw
            } else {
                #[rustfmt::skip]
                if !buffer.is_empty() {
                    let sleep = tokio::time::sleep(const { core::time::Duration::from_millis(180) });
                    let db_fut = insert_to_db(&buffer, channel.id, &mut interval, target);
                    let batch_max: Option<i32> = join!(sleep, db_fut).await.1;
                    if let Some((_, r)) = interval && r.cast_unsigned() > limit {
                        stop_point = stop_point.max(r.wrapping_sub_unsigned(limit));
                    }
                    if batch_max.is_some_and(|x| x <= stop_point) {
                        break 'outer;
                    }
                    buffer.clear();
                }
                iter.next().await
            };
            match item {
                Ok(item) => break item,
                Err(InvocationError::Rpc(RpcError {
                    code: 400, name, ..
                })) => {
                    log::error!(target: target, "channel error: {name}");
                    insert_to_db(&buffer, channel.id, &mut interval, target).await;
                    break 'outer;
                }
                Err(e) => {
                    log::error!(target: target, "{e:#?}");
                    tokio::time::sleep(const { core::time::Duration::from_secs(1) }).await;
                }
            };
        };
        let Some(message) = item else {
            insert_to_db(&buffer, channel.id, &mut interval, target).await;
            break;
        };
        let message = Message::from(message.into_inner());

        buffer.push((message.id, message));
    }

    log::info!(target: target, "span update (of {}): {:?} => {:?}", channel.id, interval_origin, interval);
}
