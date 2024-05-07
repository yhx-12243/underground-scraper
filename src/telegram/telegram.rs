use std::future::join;
use std::io::{stdin, stdout, Write};

use grammers_client::{client::bots::InvocationError, Client, Config, InitParams};
use grammers_session::{PackedChat, PackedType, Session};
use grammers_tl_types as tl;
use serde::{Deserialize, Serialize};
use tokio_postgres::types::Json;
use uscr::db::{get_connection, BB8Error, ToSqlIter};

#[allow(clippy::from_str_radix_10)] // false positive (const fn)
const API_ID: i32 = if let Ok(id) = i32::from_str_radix(env!("TG_ID"), 10) {
    id
} else {
    panic!("invalid API_ID format")
};
const API_HASH: &str = env!("TG_HASH");

const SESSION_PATH: &str = "telegram.session";

pub async fn get_client() -> anyhow::Result<Client> {
    let config = Config {
        session: Session::load_file_or_create(SESSION_PATH)?,
        api_id: API_ID,
        api_hash: API_HASH.to_owned(),
        params: InitParams::default(),
    };

    Client::connect(config).await.map_err(Into::into)
}

pub async fn login(client: &Client) -> anyhow::Result<()> {
    let mut phone = String::new();
    while !client.is_authorized().await? {
        if phone.is_empty() {
            let mut stdout = stdout();
            stdout.write_all(b"Please enter your phone: ")?;
            stdout.flush()?;
            stdin().read_line(&mut phone)?;
        }
        let token = client.request_login_code(phone.trim()).await?;

        let mut code = String::new();
        {
            let mut stdout = stdout();
            stdout.write_all(b"Please enter the code you received: ")?;
            stdout.flush()?;
            stdin().read_line(&mut code)?;
        }
        client.sign_in(&token, &code).await?;
    }
    Ok(())
}

pub fn save(client: &Client) -> std::io::Result<()> {
    client.session().save_to_file(SESSION_PATH)
}

#[derive(Debug)]
pub struct Channel {
    pub id: i64,
    pub name: String,
    pub access_hash: i64,
}

pub async fn access_channel(client: &Client, name: &str) -> anyhow::Result<Channel> {
    tracing::info!(target: "telegram-access-channel", "======== \x1b[32mACCESSING CHANNEL \x1b[36m{name}\x1b[0m ========");
    let Some(chat) = client.resolve_username(name).await? else {
        anyhow::bail!("channel {name} not found");
    };

    let PackedChat {
        id, access_hash, ..
    } = chat.pack();

    Ok(Channel {
        id,
        name: chat.username().unwrap_or_else(|| chat.name()).to_owned(),
        access_hash: access_hash.unwrap_or(0),
    })
}

pub async fn fetch_channels<C>(
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
    | Chats::Slice(messages::ChatsSlice { chats, .. })) = client.invoke(&request).await?;

    Ok(chats
        .into_iter()
        .filter_map(|chat| {
            let Chat::Channel(channel) = chat else {
                return None;
            };

            Some(Channel {
                id: channel.id,
                name: channel.username.unwrap_or(channel.title),
                access_hash: channel.access_hash.unwrap_or(0),
            })
        })
        .collect())
}

#[allow(dead_code)]
pub async fn get_channel_info(
    client: &Client,
    channel: &Channel,
) -> Result<tl::types::messages::ChatFull, InvocationError> {
    use tl::{
        enums::{messages::ChatFull::Full, InputChannel::Channel},
        types::InputChannel,
    };

    let request = tl::functions::channels::GetFullChannel {
        channel: Channel(InputChannel {
            channel_id: channel.id,
            access_hash: channel.access_hash,
        }),
    };

    let Full(result) = client.invoke(&request).await?;
    Ok(result)
}

type Media = ();
type Entity = ();

#[derive(Debug, Deserialize, Serialize)]
struct Message {
    pub out: bool,
    pub mentioned: bool,
    pub media_unread: bool,
    pub silent: bool,
    pub post: bool,
    pub from_scheduled: bool,
    pub legacy: bool,
    pub edit_hide: bool,
    pub pinned: bool,
    pub noforwards: bool,
    pub invert_media: bool,
    pub offline: bool,
    pub id: i32,
    pub from_boosts_applied: Option<i32>,
    pub via_bot_id: Option<i64>,
    pub via_business_bot_id: Option<i64>,
    pub date: i32,
    pub message: String,
    pub media: Option<Media>,
    pub entities: Option<Vec<Entity>>,
    pub views: Option<i32>,
    pub forwards: Option<i32>,
    pub edit_date: Option<i32>,
    pub post_author: Option<String>,
    pub grouped_id: Option<i64>,
    pub ttl_period: Option<i32>,
    pub quick_reply_shortcut_id: Option<i32>,
}

impl From<tl::types::Message> for Message {
    fn from(message: tl::types::Message) -> Self {
        Self {
            out: message.out,
            mentioned: message.mentioned,
            media_unread: message.media_unread,
            silent: message.silent,
            post: message.post,
            from_scheduled: message.from_scheduled,
            legacy: message.legacy,
            edit_hide: message.edit_hide,
            pinned: message.pinned,
            noforwards: message.noforwards,
            invert_media: message.invert_media,
            offline: message.offline,
            id: message.id,
            from_boosts_applied: message.from_boosts_applied,
            via_bot_id: message.via_bot_id,
            via_business_bot_id: message.via_business_bot_id,
            date: message.date,
            message: message.message,
            media: None,
            entities: None,
            views: message.views,
            forwards: message.forwards,
            edit_date: message.edit_date,
            post_author: message.post_author,
            grouped_id: message.grouped_id,
            ttl_period: message.ttl_period,
            quick_reply_shortcut_id: message.quick_reply_shortcut_id,
        }
    }
}

async fn insert_to_db(
    messages: Vec<(i32, Message)>,
    channel_id: i64,
    interval: &mut Option<(i32, i32)>,
) -> Option<i32> {
    async fn insert_to_db_inner(
        messages: Vec<(i32, Message)>,
        channel_id: i64,
    ) -> Result<(usize, usize, i32, i32), BB8Error> {
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

        let n = rows
            .iter()
            .filter(|row| !row.try_get(0).is_ok_and(|p: u32| p != 0))
            .count();

        Ok((n, len, min, max))
    }

    if messages.is_empty() {
        tracing::warn!(target: "telegram-insert-message", "empty batch");
        None
    } else {
        match insert_to_db_inner(messages, channel_id).await {
            Ok((a, b, c, d)) => {
                tracing::info!(target: "telegram-insert-message", "{a}/{b} data upserted, id range: [{c}, {d}]");
                match interval {
                    Some((l, r)) => {
                        *l = (*l).min(c);
                        *r = (*r).max(d);
                    }
                    None => *interval = Some((c, d)),
                }
                Some(d)
            }
            Err(e) => {
                tracing::error!(target: "telegram-insert-message", ?e);
                None
            }
        }
    }
}

pub async fn fetch_content(client: &Client, channel: &Channel) -> Result<(), InvocationError> {
    tracing::info!(target: "telegram-insert-message", "======== \x1b[32mFETCHING CONTENT \x1b[36m{}\x1b[0m ========", channel.id);

    let packed = PackedChat {
        ty: PackedType::Broadcast,
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
    let stop_point = interval.map_or(0, |x| x.1);

    let mut iter = client.iter_messages(packed);
    let mut buffer = Vec::new();
    let mut first = true;
    loop {
        let item = if let Some(raw) = iter.next_raw() {
            raw
        } else {
            if first {
                first = false;
            } else {
                let sleep = tokio::time::sleep(const { core::time::Duration::from_millis(180) });
                let db_fut = insert_to_db(core::mem::take(&mut buffer), channel.id, &mut interval);
                if join!(sleep, db_fut)
                    .await
                    .1
                    .is_some_and(|x| x <= stop_point)
                {
                    break;
                }
            }
            iter.next().await
        }?;
        let Some(message) = item else {
            insert_to_db(buffer, channel.id, &mut interval).await;
            break;
        };
        let message = Message::from(message.into_inner());

        buffer.push((message.id, message));
    }

    tracing::info!(target: "telegram-insert-message", "span update (of {}): {:?} => {:?}", channel.id, interval_origin, interval);

    Ok(())
}
