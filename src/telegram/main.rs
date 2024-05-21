#![feature(
    const_int_from_str,
    stmt_expr_attributes,
    future_join,
    let_chains,
    ptr_sub_ptr,
    string_deref_patterns,
    try_blocks,
    yeet_expr
)]

use hashbrown::{HashMap, HashSet};

mod extract;
mod telegram;

async fn insert_channels<C, I>(channels: C, invite_map: I) -> Result<(), uscr::db::BB8Error>
where
    C: Iterator<Item = telegram::Channel>,
    I: Iterator<Item = (String, i64)>,
{
    use uscr::db::get_connection;

    const SQL: &str = "insert into telegram.channel (id, name, min_message_id, max_message_id, access_hash, last_fetch) values ($1, $2, 0, 0, $3, (now() at time zone 'UTC') - interval '1 day') on conflict (id) do update set name = excluded.name, access_hash = excluded.access_hash";

    const SQL_I: &str = "insert into telegram.invite (hash, channel_id) values ($1, $2) on conflict (hash) do update set channel_id = excluded.channel_id";

    let mut conn = get_connection().await?;
    let stmt = conn.prepare_static(SQL.into()).await?;
    let stmti = conn.prepare_static(SQL_I.into()).await?;
    let txn = conn.transaction().await?;

    let mut n = 0;
    let mut N = 0;
    for channel in channels {
        match txn
            .execute(&stmt, &[&channel.id, &&*channel.name, &channel.access_hash])
            .await
        {
            Ok(r) => n += r,
            Err(e) => tracing::error!(target: "telegram-insert-channel", ?e),
        }
        N += 1;
    }

    let mut ni = 0;
    let mut Ni = 0;
    for (hash, id) in invite_map {
        match txn.execute(&stmti, &[&hash, &id]).await {
            Ok(r) => ni += r,
            Err(e) => tracing::error!(target: "telegram-insert-invite", ?e),
        }
        Ni += 1;
    }

    txn.commit().await?;

    tracing::info!(target: "telegram-insert-channel", "{n}/{N} records upserted, {ni}/{Ni} invites upserted.");
    Ok(())
}

async fn get_all_channels_from_db() -> Result<Vec<telegram::Channel>, uscr::db::BB8Error> {
    use uscr::db::get_connection;

    const SQL: &str = "select id, name, access_hash from telegram.channel where last_fetch < '3000-1-1' order by last_fetch";

    let mut conn = get_connection().await?;
    let stmt = conn.prepare_static(SQL.into()).await?;
    let rows = conn.query(&stmt, &[]).await?;

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let id = row.try_get(0).ok()?;
            let name = row.try_get::<_, &str>(1).ok()?;
            let access_hash = row.try_get(2).ok()?;
            Some(telegram::Channel {
                id,
                name: name.into(),
                access_hash,
            })
        })
        .collect())
}

#[derive(clap::Parser)]
struct Args {
    #[command(subcommand)]
    command: Commands,
    #[arg(
        short,
        long,
        default_value = "telegram.session",
        value_name = "file",
        help = "The file that stores session"
    )]
    session: std::path::PathBuf,
}

#[derive(clap::Subcommand)]
enum Commands {
    Ping {
        #[arg(short, long, num_args=1.., required=true)]
        channels: Vec<String>,
    },
    Content {
        #[arg(short, long, num_args=1.., value_parser=clap::value_parser!(i64).range(0..))]
        channels: Vec<i64>,
        #[arg(short, long, default_value_t = 10240)]
        limit: u32,
    },
    Extract {
        #[arg(short, long, default_value = "ids.txt")]
        save: std::path::PathBuf,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use clap::Parser;

    pretty_env_logger::init_timed();
    uscr::db::init_db().await;

    let args = Args::parse();

    let client = telegram::get_client(&args.session).await?;
    telegram::login(&client).await?;
    telegram::save(&client, &args.session)?;

    match args.command {
        Commands::Ping {
            channels: raw_channels,
        } => {
            let mut channels = HashMap::with_capacity(raw_channels.len());
            let mut ids = HashSet::with_capacity(raw_channels.len());
            let mut invite = HashMap::with_capacity(raw_channels.len());
            for raw_channel in raw_channels {
                if let Ok(id) = raw_channel.parse() {
                    ids.insert(id);
                    continue;
                }
                match telegram::access_channel(&client, &raw_channel).await {
                    Ok(channel) => {
                        invite.insert(raw_channel, channel.id);
                        channels.insert(channel.id, channel);
                        continue;
                    }
                    Err(e) => tracing::error!(?e),
                }
                match telegram::access_invite(&client, &raw_channel).await {
                    Ok(channel) => {
                        invite.insert(raw_channel, channel.id);
                        channels.insert(channel.id, channel);
                        continue;
                    }
                    Err(e) => tracing::error!(?e),
                }
            }
            for channel in telegram::fetch_channels(
                &client,
                ids.into_iter().filter(|id| !channels.contains_key(id)),
            )
            .await?
            {
                channels.insert(channel.id, channel);
            }
            tracing::info!("{channels:#?}");
            insert_channels(channels.into_values(), invite.into_iter()).await?;
        }
        Commands::Content {
            channels: channels_filt,
            limit,
        } => {
            let mut channels = get_all_channels_from_db().await?;
            if !channels_filt.is_empty() {
                let filt = channels_filt.into_iter().collect::<HashSet<i64>>();
                channels.retain(|channel| filt.contains(&channel.id));
            }
            for channel in channels {
                telegram::fetch_content(&client, &channel, limit).await;
            }
        }
        Commands::Extract { save } => {
            let map = extract::generate_user_id_map().await?;

            let mut inspector = extract::Inspector::new(
                map,
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(save)?,
            );
            inspector.extract_content().await?;
        }
    }

    telegram::save(&client, &args.session).map_err(Into::into)
}
