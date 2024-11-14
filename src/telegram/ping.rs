use compact_str::{format_compact, CompactString};
use either::Either::{self, Left, Right};
use grammers_client::InvocationError;
use grammers_mtsender::RpcError;
use grammers_session::{PackedChat, PackedType};
use grammers_tl_types as tl;
use hashbrown::{HashMap, HashSet};
use parking_lot::Mutex;
use tokio_postgres::Client as DBClient;
use tokio_postgres::types::Json;
use unicase::UniCase;

use crate::{
    db::DBWrapper,
    telegram::{BotCommand, COMMAND_LIST, Channel, User, client::Client},
};

pub fn separate_id_and_names(
    raw: Vec<CompactString>,
    filter_out: &HashMap<UniCase<CompactString>, i64>,
) -> (HashSet<i64>, HashSet<UniCase<CompactString>>) {
    let filter_out_id = filter_out.values().copied().collect::<HashSet<_>>();

    let mut ids = HashSet::with_capacity(raw.len());
    let mut name_or_hashes = HashSet::with_capacity(raw.len());

    for entry in raw {
        if let Ok(id) = entry.parse() {
            if !filter_out_id.contains(&id) {
                ids.insert(id);
            }
        } else {
            let entry = UniCase::new(entry);
            if !filter_out.contains_key(&entry) {
                name_or_hashes.insert(entry);
            }
        }
    }

    (ids, name_or_hashes)
}

pub async fn work<'a, I>(
    keys: Vec<CompactString>,
    clients: I,
    conn: &mut DBClient,
) -> (Vec<Channel>, Vec<User>)
where
    I: Iterator<Item = (&'a i32, &'a Client)> + Send,
{
    const SQL_INVITE: &str = "insert into telegram.invite (hash, channel_id, type, description) values ($1, $2, $3, $4) on conflict (hash) do update set channel_id = excluded.channel_id, type = excluded.type, description = excluded.description";
    const SQL_CMDS: &str = "insert into telegram.interaction (bot_id, message_id, request, response) values ($1, $2, $3, $4) on conflict (bot_id, message_id) do update set request = excluded.request, response = excluded.response";

    let keys = Mutex::new(keys);
    let stmt_invite = conn.prepare_static(SQL_INVITE.into()).await.unwrap();
    let stmt_cmds = conn.prepare_static(SQL_CMDS.into()).await.unwrap();
    let db = DBWrapper {
        conn,
        stmts: [&stmt_invite, &stmt_cmds],
    };
    let futs = clients.map(|(id, client)| into_future(*id, client, &keys, db));
    let folded = futures_util::future::join_all(futs).await;

    let (mut channels, mut users) = (Vec::new(), Vec::new());
    for (mut c, mut u) in folded {
        channels.append(&mut c);
        users.append(&mut u);
    }
    (channels, users)
}

async fn get_description(
    client: &Client,
    ty: PackedType,
    id: i64,
    access_hash: i64,
    target: &str,
) -> (String, Option<Vec<BotCommand>>) {
    if ty == PackedType::User || ty == PackedType::Bot {
        use tl::{
            enums::{
                BotInfo as EBotInfo, BotMenuButton as EBotMenuButton, UserFull as EUserFull,
                users::UserFull as EUUserFull,
            },
            types::{BotMenuButton as TBotMenuButton, users::UserFull as TUUserFull},
        };

        let request = tl::functions::users::GetFullUser {
            id: tl::enums::InputUser::User(tl::types::InputUser { user_id: id, access_hash })
        };
        match client.inner.invoke(&request).await {
            Ok(EUUserFull::Full(TUUserFull { full_user: EUserFull::Full(u), .. })) => {
                let mut base = u.about.unwrap_or_default();
                if let Some(EBotInfo::Info(bot_info)) = u.bot_info {
                    if let Some(ref desc) = bot_info.description {
                        base.push_str("\n\n");
                        base.push_str(desc);
                    }
                    if let Some(EBotMenuButton::Button(TBotMenuButton { ref text, ref url })) = bot_info.menu_button {
                        base.push_str("\n\n[");
                        base.push_str(text);
                        base.push_str("](");
                        base.push_str(url);
                        base.push(')');
                    }
                    if let Some(commands) = bot_info.commands {
                        return (base, Some(commands.into_iter().map(Into::into).collect()));
                    }
                }

                (base, None)
            }
            Err(e) => {
                log::error!(target: target, "get \x1b[35mdescription of [{ty}] {id}\x1b[0m err: {e:?}");
                (e.to_string(), None)
            }
        }
    } else {
        use tl::{
            enums::{ChatFull as EChatFull, messages::ChatFull as EMChatFull},
            types::messages::ChatFull as TMChatFull,
        };

        let response = if ty == PackedType::Chat {
            let request = tl::functions::messages::GetFullChat { chat_id: id };
            client.inner.invoke(&request).await
        } else {
            let request = tl::functions::channels::GetFullChannel {
                channel: tl::enums::InputChannel::Channel(tl::types::InputChannel { channel_id: id, access_hash })
            };
            client.inner.invoke(&request).await
        };

        (match response {
            Ok(EMChatFull::Full(TMChatFull { full_chat: EChatFull::Full(c), .. })) => c.about,
            Ok(EMChatFull::Full(TMChatFull { full_chat: EChatFull::ChannelFull(c), .. })) => c.about,
            Err(e) => {
                log::error!(target: target, "get \x1b[35mdescription of [{ty}] {id}\x1b[0m err: {e:?}");
                e.to_string()
            }
        }, None)
    }
}

async fn access_channel(
    client: &Client,
    name: &str,
    db: DBWrapper<'_, 2>,
    target: &str,
) -> anyhow::Result<Either<Channel, User>> {
    use grammers_client::types::Chat::{Channel as Chan, Group, User};

    log::info!(target: target, "======== \x1b[32mACCESSING CHANNEL \x1b[36m{name}\x1b[0m ========");
    let chat = match client.inner.resolve_username(name).await {
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

    let PackedChat { id, access_hash, ty } = chat.pack();
    let (description, commands) = get_description(client, ty, id, access_hash.unwrap_or(0), target).await;

    db.conn.execute(db.stmts[0], &[
        &name,
        &id,
        &(match chat {
            Chan(_) => b'C',
            Group(_) => b'G',
            User(_) => b'U',
        }.cast_signed()),
        &description,
    ]).await?;

    if let Some(commands) = commands {
        db.conn.execute(db.stmts[1], &[&id, &-1i32, &COMMAND_LIST, &Json(commands)]).await?;
    }

    let peer = Channel {
        id,
        name: chat
            .username()
            .or_else(|| chat.name())
            .map_or_else(|| format_compact!("channel#{id}"), CompactString::new),
        access_hash: access_hash.unwrap_or(0),
        app_id: 0,
    };

    Ok(if matches!(chat, User(_)) {
        Right(crate::telegram::User {
            peer,
            hash_name: name.into(),
        })
    } else {
        Left(peer)
    })
}

async fn access_invite(
    client: &Client,
    name: &str,
    db: DBWrapper<'_, 2>,
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
            "\x1b[37m{name} caused by \x1b[33m{caused_by:?}\x1b[37m, wait \x1b[33m{value:?}\x1b[0m"
        ),
        Err(e) => return Err(e.into()),
    };

    let (ChatInvite::Already(ChatInviteAlready { chat }) | ChatInvite::Peek(ChatInvitePeek { chat, .. })) = r
    else {
        anyhow::bail!("Cannot get entity from a channel (or group) that you are not part of. Join the group and retry")
    };

    match chat {
        Chat::Channel(tl::types::Channel { id, access_hash, title, username, .. }) => {
            let description = get_description(client, PackedType::Megagroup, id, access_hash.unwrap_or(0), target).await.0;
            db.conn
                .execute(
                    db.stmts[0],
                    &[&name, &id, &(b'C'.cast_signed()), &description],
                )
                .await?;

            Ok(Channel {
                id,
                name: username.unwrap_or(title).into(),
                access_hash: access_hash.unwrap_or(0),
                app_id: 0,
            })
        }
        Chat::Chat(tl::types::Chat { id, title, .. }) => {
            let description = get_description(client, PackedType::Chat, id, 0, target).await.0;
            db.conn
                .execute(
                    db.stmts[0],
                    &[&name, &id, &(b'G'.cast_signed()), &description],
                )
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
    keys: &Mutex<Vec<CompactString>>,
    db: DBWrapper<'_, 2>,
) -> (Vec<Channel>, Vec<User>) {
    let mut channels = Vec::new();
    let mut users = Vec::new();
    let target_access_channel = format!("telegram-access-channel({id})");
    let target_access_invite = format!("telegram-access-invite({id})");

    loop {
        let Some(key) = keys.lock().pop() else {
            return (channels, users);
        };

        match access_channel(client, &key, db, &target_access_channel).await {
            Ok(Left(mut channel)) => {
                channel.app_id = id;
                channels.push(channel);
                continue;
            }
            Ok(Right(mut user)) => {
                user.peer.app_id = id;
                users.push(user);
                continue;
            }
            Err(e) => log::error!(target: &target_access_channel, "{e:?}"),
        }

        match access_invite(client, &key, db, &target_access_invite).await {
            Ok(mut channel) => {
                channel.app_id = id;
                channels.push(channel);
                continue;
            }
            Err(e) => log::error!(target: &target_access_invite, "{e:?}"),
        }
    }
}
