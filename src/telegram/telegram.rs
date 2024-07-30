pub mod client;
mod types;

pub use types::BotCommand;

use std::{
    fs::File,
    future::join,
    io::{self, BufReader},
    path::Path,
    time::Duration,
};

use client::{Client, InitConfig};
use compact_str::CompactString;
use grammers_client::InputMessage;
use grammers_mtsender::{InvocationError, RpcError};
use grammers_session::PackedChat;
use grammers_tl_types as tl;
use tokio::{sync::oneshot, time::timeout};
use tokio_postgres::types::Json;
use types::Message;
use uscr::{
    db::{DBResult, ToSqlIter},
    util::xmax_to_success,
};

use crate::db::DBWrapper;

pub fn parse_config(file: &Path) -> io::Result<Vec<InitConfig>> {
    let file = File::open(file)?;
    let reader = BufReader::new(file);
    serde_json::from_reader(reader).map_err(Into::into)
}

#[repr(transparent)]
#[derive(Debug)]
pub struct User(pub Channel);

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
    C: Iterator<Item = i64> + Send,
{
    use tl::{
        enums::{messages::Chats, Chat, InputChannel::Channel as Ch},
        types::{messages, InputChannel},
    };

    let request = tl::functions::channels::GetChannels {
        id: channels.map(|channel_id| Ch(InputChannel { channel_id, access_hash: 0 })).collect(),
    };

    let (Chats::Chats(messages::Chats { chats }) | Chats::Slice(messages::ChatsSlice { chats, .. })) = client.inner.invoke(&request).await?;

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
    db: DBWrapper<'_, 3>,
) -> Option<i32> {
    async fn insert_to_db_inner(
        messages: &[(i32, Message)],
        channel_id: i64,
        db: DBWrapper<'_, 3>,
    ) -> DBResult<(usize, usize, i32, i32)> {
        let len = messages.len();
        let min = messages.iter().fold(i32::MAX, |x, y| x.min(y.0));
        let max = messages.iter().fold(i32::MIN, |x, y| x.max(y.0));

        let rows = db
            .conn
            .query(
                db.stmts[1],
                &[
                    &ToSqlIter(messages.iter().map(|x| x.0)),
                    &channel_id,
                    &ToSqlIter(messages.iter().map(|x| Json(&x.1))),
                ],
            )
            .await?;

        Ok((xmax_to_success(rows.iter()), len, min, max))
    }

    if messages.is_empty() {
        log::warn!(target: target, "empty batch");
        None
    } else {
        match insert_to_db_inner(messages, channel_id, db).await {
            Ok((succ, len, min, max)) => {
                log::info!(target: target, "{succ}/{len} data upserted, id range: [{min}, {max}]");
                let inner = match interval {
                    Some(inner) => {
                        inner.0 = inner.0.min(min);
                        inner.1 = inner.1.max(max);
                        inner
                    }
                    None => interval.insert((min, max)),
                };
                if let Err(e) = db.conn.execute(db.stmts[2], &[&inner.0, &inner.1, &channel_id]).await {
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

pub async fn fetch_content(
    client: &Client,
    channel: &Channel,
    limit: u32,
    target: &str,
    db: DBWrapper<'_, 3>,
) {
    log::info!(target: target, "======== \x1b[32mFETCHING CONTENT \x1b[36m{}\x1b[0m ========", channel.id);

    let packed = PackedChat {
        ty: grammers_session::PackedType::Broadcast,
        id: channel.id,
        access_hash: (channel.access_hash != 0).then_some(channel.access_hash),
    };

    // compute stop line
    let interval_origin: Option<(i32, i32)> = try {
        let row = db.conn.query_one(db.stmts[0], &[&channel.id]).await.ok()?;
        let min = row.try_get(0).ok()?;
        let max = row.try_get(1).ok()?;

        if min == 0 && max == 0 {
            do yeet;
        } else {
            (min, max)
        }
    };
    let mut interval = interval_origin;
    let mut stop_point = interval.map_or(0, |(_, max)| max);

    let mut jumping = interval
        .is_some_and(|(min, max)| min > 1 && limit > 1 && (max - min).cast_unsigned() < limit - 1);

    let mut iter = client.inner.iter_messages(packed);
    let mut buffer = Vec::with_capacity(100);
    'outer: loop {
        let mut n_err = 0;
        let item = loop {
            let item = if let Some(raw) = iter.next_raw() {
                raw
            } else {
                #[rustfmt::skip]
                if !buffer.is_empty() {
                    let sleep = tokio::time::sleep(const { core::time::Duration::from_millis(180) });
                    let db_fut = insert_to_db(&buffer, channel.id, &mut interval, target, db);
                    let batch_max: Option<i32> = join!(sleep, db_fut).await.1;
                    let (l, r) = interval.expect("interval shouldn't be None after insert");
                    let l_i = r.cast_unsigned().checked_sub(limit).and_then(|x| x.try_into().ok()).unwrap_or(0);
                    stop_point = stop_point.max(l_i);
                    if batch_max.is_some_and(|x| x <= stop_point) {
                        if !jumping {
                            break 'outer;
                        }
                        jumping = false;
                        stop_point = l_i;
                        iter = iter.offset_id(l - 1);
                    }
                    buffer.clear();
                }
                iter.next().await
            };
            match item {
                Ok(item) => break item,
                Err(InvocationError::Rpc(RpcError { code: 400, name, caused_by, value })) => {
                    log::error!(target: target, "channel error: {name} caused by \x1b[33m{caused_by:?}\x1b[0m, with value \x1b[33m{value:?}\x1b[0m");
                    insert_to_db(&buffer, channel.id, &mut interval, target, db).await;
                    break 'outer;
                }
                Err(e) => {
                    log::error!(target: target, "{e:#?}");
                    n_err += 1;
                    if n_err == 5 {
                        log::error!(target: target, "channel error too many times: {e:#?}, breaking");
                        insert_to_db(&buffer, channel.id, &mut interval, target, db).await;
                        break 'outer;
                    }
                    tokio::time::sleep(const { core::time::Duration::from_secs(1) }).await;
                }
            }
        };
        let Some(message) = item else {
            insert_to_db(&buffer, channel.id, &mut interval, target, db).await;
            break;
        };
        buffer.push((message.raw.id, message.raw.into()));
    }

    log::info!(target: target, "span update (of {}): {:?} => {:?}", channel.id, interval_origin, interval);
}

async fn interact_inner(
    client: &Client,
    chat: PackedChat,
    text: &str,
    target: &str,
) -> Option<Message> {
    if let Err(e) = client.inner.send_message(chat, InputMessage::text(text)).await {
        log::error!(target: target, "sending {text}: {e:#?}");
        return None;
    }

    let (tx, rx) = oneshot::channel();
    client.register(chat.id, tx);
    let fut = timeout(const { Duration::from_secs(10) }, rx);

    match fut.await {
        Ok(Ok(resp)) => Some(resp.raw.into()),
        Ok(Err(e)) => {
            log::error!(target: target, "receiving {text}: {e:#?}");
            None
        }
        Err(_) => {
            log::error!(target: target, "receiving {text}: no response");
            None
        }
    }
}

pub async fn interact_bot(client: &Client, bot: &User, target: &str, db: DBWrapper<'_, 1>) {
    log::info!(target: target, "======== \x1b[1;34mINTERACTING BOT \x1b[36m{}\x1b[0m ========", bot.0.id);

    let packed = PackedChat {
        ty: grammers_session::PackedType::Bot,
        id: bot.0.id,
        access_hash: (bot.0.access_hash != 0).then_some(bot.0.access_hash),
    };

    if let Some(resp_start) = interact_inner(client, packed.clone(), "/start", target).await {
        let id = resp_start.id;
        if let Err(e) = db.conn.execute(db.stmts[0], &[&bot.0.id, &id, &"/start", &Json(resp_start)]).await {
            log::error!(target: target, "db(insert /start): {e:?}");
        }
    }

    if let Some(resp_help) = interact_inner(client, packed, "/help", target).await {
        let id = resp_help.id;
        if let Err(e) = db.conn.execute(db.stmts[0], &[&bot.0.id, &id, &"/help", &Json(resp_help)]).await {
            log::error!(target: target, "db(insert /help): {e:?}");
        }
    }
}
