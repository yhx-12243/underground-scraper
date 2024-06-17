use grammers_client::InvocationError;
use grammers_mtsender::RpcError;
use grammers_session::PackedChat;
use grammers_tl_types as tl;
use parking_lot::Mutex;
use tokio::sync::Mutex as AsyncMutex;
use tokio_postgres::Client as DBClient;

use crate::telegram::{client::Client, Channel};

pub async fn work<'a, I>(
    keys: Vec<String>,
    clients: I,
    conn: &mut DBClient,
) -> impl Iterator<Item = Channel>
where
    I: Iterator<Item = (&'a i32, &'a Client)>,
{
    let keys = Mutex::new(keys);
    let conn = AsyncMutex::new(conn);

    let futs = clients.map(|(id, client)| into_future(*id, client, &keys, &conn));
    let folded = futures_util::future::join_all(futs).await;
    folded.into_iter().flatten()
}

const SQL_INVITE: &str = "insert into telegram.invite (hash, channel_id, type) values ($1, $2, $3) on conflict (hash) do update set channel_id = excluded.channel_id";

async fn access_channel(
    client: &Client,
    name: &str,
    conn: &AsyncMutex<&mut DBClient>,
    target: &str,
) -> anyhow::Result<Channel> {
    use grammers_client::types::Chat::{Group, User};

    log::info!(target: target, "======== \x1b[32mACCESSING CHANNEL \x1b[36m{name}\x1b[0m ========");
    let chat = match client.inner.resolve_username(name).await {
        Ok(Some(User(user))) => {
            let mut conn = conn.lock().await;
            let stmt = conn.prepare_static(SQL_INVITE.into()).await?;
            conn.execute(&stmt, &[&name, &user.id(), &(b'U'.cast_signed())])
                .await?;
            anyhow::bail!("{name} is a user");
        }
        Ok(Some(c)) => c,
        Ok(None) => anyhow::bail!("channel {name} not found"),
        Err(InvocationError::Rpc(RpcError {
            code: 420,
            name,
            value,
            caused_by,
        })) => anyhow::bail!(
            "{name} caused by \x1b[33m{caused_by:?}\x1b[0m, wait \x1b[33m{value:?}\x1b[0m"
        ),
        Err(e) => return Err(e.into()),
    };

    let PackedChat {
        id, access_hash, ..
    } = chat.pack();

    let mut conn = conn.lock().await;
    let stmt = conn.prepare_static(SQL_INVITE.into()).await?;
    conn.execute(
        &stmt,
        &[
            &name,
            &id,
            &(if matches!(chat, Group(_)) { b'G' } else { b'C' }.cast_signed()),
        ],
    )
    .await?;

    Ok(Channel {
        id,
        name: chat.username().unwrap_or_else(|| chat.name()).into(),
        access_hash: access_hash.unwrap_or(0),
        app_id: 0,
    })
}

async fn access_invite(
    client: &Client,
    name: &str,
    conn: &AsyncMutex<&mut DBClient>,
    target: &str,
) -> anyhow::Result<Channel> {
    use tl::{
        enums::{Chat, ChatInvite},
        types::{ChatInviteAlready, ChatInvitePeek},
    };

    log::info!(target: target, "======== \x1b[32mACCESSING INVITE \x1b[36m{name}\x1b[0m ========");

    let request = tl::functions::messages::CheckChatInvite {
        hash: name.to_owned(),
    };

    let r = match client.inner.invoke(&request).await {
        Ok(r) => r,
        Err(InvocationError::Rpc(RpcError {
            code: 420,
            name,
            value,
            caused_by,
        })) => anyhow::bail!(
            "{name} caused by \x1b[33m{caused_by:?}\x1b[0m, wait \x1b[33m{value:?}\x1b[0m"
        ),
        Err(e) => return Err(e.into()),
    };

    let mut conn = conn.lock().await;
    let stmt = conn.prepare_static(SQL_INVITE.into()).await?;

    let (ChatInvite::Already(ChatInviteAlready { chat }) | ChatInvite::Peek(ChatInvitePeek { chat, .. })) = r
    else {
        anyhow::bail!("Cannot get entity from a channel (or group) that you are not part of. Join the group and retry")
    };

    match chat {
        Chat::Channel(tl::types::Channel {
            id,
            access_hash,
            title,
            username,
            ..
        }) => {
            conn.execute(&stmt, &[&name, &id, &(b'C'.cast_signed())])
                .await?;

            Ok(Channel {
                id,
                name: username.unwrap_or(title).into(),
                access_hash: access_hash.unwrap_or(0),
                app_id: 0,
            })
        }
        Chat::Chat(tl::types::Chat { id, title, .. }) => {
            conn.execute(&stmt, &[&name, &id, &(b'G'.cast_signed())])
                .await?;

            Ok(Channel {
                id,
                name: title.into(),
                access_hash: 0,
                app_id: 0,
            })
        }
        _ => Err(anyhow::anyhow!("type mismatch")),
    }
}

async fn into_future(
    id: i32,
    client: &Client,
    keys: &Mutex<Vec<String>>,
    conn: &AsyncMutex<&mut DBClient>,
) -> Vec<Channel> {
    let mut channels = Vec::new();
    let target_access_channel = format!("telegram-access-channel({id})");
    let target_access_invite = format!("telegram-access-invite({id})");

    loop {
        let Some(key) = keys.lock().pop() else {
            return channels;
        };

        match access_channel(client, &key, conn, &target_access_channel).await {
            Ok(mut channel) => {
                channel.app_id = id;
                channels.push(channel);
                continue;
            }
            Err(e) => log::error!(target: &target_access_channel, "{e:?}"),
        }

        match access_invite(client, &key, conn, &target_access_invite).await {
            Ok(mut channel) => {
                channel.app_id = id;
                channels.push(channel);
                continue;
            }
            Err(e) => log::error!(target: &target_access_invite, "{e:?}"),
        }
    }
}
