#![feature(
    const_int_from_str,
    future_join,
    hint_assert_unchecked,
    string_deref_patterns,
    try_blocks,
)]

mod telegram;

async fn insert_channels(channels: &[telegram::Channel]) -> Result<(), uscr::db::BB8Error> {
    use uscr::db::get_connection;

    const SQL: &str = "insert into telegram.channel (id, name, min_message_id, max_message_id, access_hash) values ($1, $2, 0, 0, $3) on conflict (id) do update set name = excluded.name, access_hash = excluded.access_hash";

    let mut conn = get_connection().await?;
    let stmt = conn.prepare_static(SQL.into()).await?;
    let txn = conn.transaction().await?;

    let mut n = 0;
    for channel in channels {
        match txn
            .execute(&stmt, &[&channel.id, &channel.name, &channel.access_hash])
            .await
        {
            Ok(r) => n += r,
            Err(e) => tracing::error!(target: "telegram-insert-channel", ?e),
        }
    }
    txn.commit().await?;

    tracing::info!(target: "telegram-insert-channel", "{n}/{} records upserted.", channels.len());
    Ok(())
}

async fn get_all_channels_from_db() -> Result<Vec<telegram::Channel>, uscr::db::BB8Error> {
    use uscr::db::get_connection;

    const SQL: &str = "select id, name, access_hash from telegram.channel";

    let mut conn = get_connection().await?;
    let stmt = conn.prepare_static(SQL.into()).await?;
    let rows = conn.query(&stmt, &[]).await?;

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let id = row.try_get(0).ok()?;
            let name = row.try_get(1).ok()?;
            let access_hash = row.try_get(2).ok()?;
            Some(telegram::Channel {
                id,
                name,
                access_hash,
            })
        })
        .collect())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pretty_env_logger::init_timed();
    uscr::db::init_db().await;

    let client = telegram::get_client().await?;
    telegram::login(&client).await?;
    telegram::save(&client)?;

    let arg = std::env::args_os()
        .nth(1)
        .map(std::ffi::OsString::into_string);

    let id: Option<i64> = try {
        arg.as_ref()?
            .as_ref()
            .ok()?
            .parse::<u64>()
            .ok()?
            .try_into()
            .ok()?
    };

    if let Some(id) = id {
        let channels = telegram::fetch_channels(&client, core::iter::once(id)).await?;
        tracing::info!("{channels:?}");
        insert_channels(&channels).await?;
    } else if let Some(Ok("content")) = arg {
        let mut channels = get_all_channels_from_db().await?;
        let mut thread_rng = rand::thread_rng();
        rand::seq::SliceRandom::shuffle(&mut *channels, &mut thread_rng);
        tracing::info!("{channels:?}");

        for channel in channels {
            telegram::fetch_content(&client, &channel).await?;
        }
    }

    Ok(())
}
