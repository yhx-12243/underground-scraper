use core::pin::pin;

use compact_str::CompactString;
use futures_util::TryStreamExt;
use hashbrown::HashMap;
use tokio_postgres::{Client, Statement, types::ToSql};
use unicase::UniCase;
use uscr::db::DBResult;

use crate::telegram::{Channel, User};

#[derive(Clone, Copy)]
pub struct DBWrapper<'a, const N: usize> {
    pub conn: &'a Client,
    pub stmts: [&'a Statement; N],
}

pub async fn insert_channels<C>(channels: C, conn: &mut Client) -> DBResult<()>
where
    C: Iterator<Item = Channel> + Send,
{
    const SQL: &str = "insert into telegram.channel (id, name, min_message_id, max_message_id, access_hash, last_fetch, app_id) values ($1, $2, 0, 0, $3, (now() at time zone 'UTC') - interval '1 day', $4) on conflict (id) do update set name = excluded.name, access_hash = excluded.access_hash, app_id = excluded.app_id";

    let stmt = conn.prepare_static(SQL.into()).await?;
    let txn = conn.transaction().await?;

    let mut n = 0;
    let mut N = 0;
    for channel in channels {
        match txn.execute(&stmt, &[
            &channel.id,
            &&*channel.name,
            &channel.access_hash,
            &channel.app_id,
        ]).await {
            Ok(r) => n += r,
            Err(e) => tracing::error!(target: "telegram-insert-channel", ?e),
        }
        N += 1;
    }

    txn.commit().await?;

    tracing::info!(target: "telegram-insert-channel", "{n}/{N} records upserted.");
    Ok(())
}

pub async fn insert_users<C>(users: C, conn: &mut Client) -> DBResult<()>
where
    C: Iterator<Item = User> + Send,
{
    const SQL: &str = "insert into telegram.bots (id, name, access_hash, app_id) values ($1, $2, $3, $4) on conflict (id) do update set name = excluded.name, access_hash = excluded.access_hash, app_id = excluded.app_id";

    let stmt = conn.prepare_static(SQL.into()).await?;
    let txn = conn.transaction().await?;

    let mut n = 0;
    let mut N = 0;
    for user in users {
        match txn.execute(&stmt, &[
            &user.peer.id,
            &&*user.peer.name,
            &user.peer.access_hash,
            &user.peer.app_id,
        ]).await {
            Ok(r) => n += r,
            Err(e) => tracing::error!(target: "telegram-insert-user", ?e),
        }
        N += 1;
    }

    txn.commit().await?;

    tracing::info!(target: "telegram-insert-user", "{n}/{N} records upserted.");
    Ok(())
}

async fn get_all_peers_from_db_inner(sql: &'static str, conn: &mut Client) -> DBResult<Vec<Channel>> {
    let stmt = conn.prepare_static(sql.into()).await?;
    let rows = conn.query(&stmt, &[]).await?;

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let id = row.try_get(0).ok()?;
            let name = row.try_get::<_, &str>(1).ok()?;
            let access_hash = row.try_get(2).ok()?;
            let app_id = row.try_get(3).ok()?;
            Some(Channel {
                id,
                name: name.into(),
                access_hash,
                app_id,
            })
        })
        .collect())
}

pub fn get_all_channels_from_db(conn: &mut Client) -> impl Future<Output = DBResult<Vec<Channel>>> {
    get_all_peers_from_db_inner("select id, name, access_hash, app_id from telegram.channel order by last_fetch", conn)
}

pub async fn get_all_bots_from_db(conn: &mut Client) -> DBResult<Vec<User>> {
    match get_all_peers_from_db_inner("select id, name, access_hash, app_id from telegram.bots", conn).await {
        Ok(r) => Ok(r.into_iter().map(Into::into).collect()),
        Err(e) => Err(e),
    }
}

pub async fn get_searched_peers(conn: &mut Client) -> DBResult<HashMap<UniCase<CompactString>, i64>> {
    const SQL: &str =
        "select channel_id, hash from telegram.invite union all \
         select id, name from telegram.channel union all \
         select id, name from telegram.bots";

    let stmt = conn.prepare_static(SQL.into()).await?;
    let stream = conn.query_raw(&stmt, core::iter::empty::<&dyn ToSql>()).await?;
    let mut stream = pin!(stream);
    let mut result = HashMap::new();
    while let Some(row) = stream.try_next().await? {
        let id = row.try_get(0)?;
        let name = row.try_get::<_, &str>(1)?;
        result.insert(UniCase::new(name.into()), id);
    }
    Ok(result)
}
