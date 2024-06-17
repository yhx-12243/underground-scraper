#![feature(
    const_int_from_str,
    stmt_expr_attributes,
    future_join,
    integer_sign_cast,
    let_chains,
    os_str_display,
    ptr_sub_ptr,
    string_deref_patterns,
    try_blocks,
    yeet_expr
)]

mod extract;
mod ping;
mod telegram;

async fn insert_channels<C>(
    channels: C,
    conn: &mut tokio_postgres::Client,
) -> Result<(), uscr::db::BB8Error>
where
    C: Iterator<Item = telegram::Channel>,
{
    const SQL: &str = "insert into telegram.channel (id, name, min_message_id, max_message_id, access_hash, last_fetch, app_id) values ($1, $2, 0, 0, $3, (now() at time zone 'UTC') - interval '1 day', $4) on conflict (id) do update set name = excluded.name, access_hash = excluded.access_hash, app_id = excluded.app_id";

    let stmt = conn.prepare_static(SQL.into()).await?;
    let txn = conn.transaction().await?;

    let mut n = 0;
    let mut N = 0;
    for channel in channels {
        match txn
            .execute(
                &stmt,
                &[
                    &channel.id,
                    &&*channel.name,
                    &channel.access_hash,
                    &channel.app_id,
                ],
            )
            .await
        {
            Ok(r) => n += r,
            Err(e) => tracing::error!(target: "telegram-insert-channel", ?e),
        }
        N += 1;
    }

    txn.commit().await?;

    tracing::info!(target: "telegram-insert-channel", "{n}/{N} records upserted.");
    Ok(())
}

async fn get_all_channels_from_db() -> Result<Vec<telegram::Channel>, uscr::db::BB8Error> {
    use uscr::db::get_connection;

    const SQL: &str =
        "select id, name, access_hash, app_id from telegram.channel order by last_fetch";

    let mut conn = get_connection().await?;
    let stmt = conn.prepare_static(SQL.into()).await?;
    let rows = conn.query(&stmt, &[]).await?;

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let id = row.try_get(0).ok()?;
            let name = row.try_get::<_, &str>(1).ok()?;
            let access_hash = row.try_get(2).ok()?;
            let app_id = row.try_get(3).ok()?;
            Some(telegram::Channel {
                id,
                name: name.into(),
                access_hash,
                app_id,
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
        default_value = "./sessions",
        value_name = "dir",
        help = "The directory that stores sessions"
    )]
    session: std::path::PathBuf,
    #[arg(
        short,
        long,
        default_value = "telegram.json",
        value_name = "file",
        help = "The config file"
    )]
    config: std::path::PathBuf,
    #[arg(
        long,
        default_value_t = 192,
        value_name = "seconds",
        help = "flood sleep threshold"
    )]
    flood_sleep_threshold: u32,
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
#[allow(clippy::too_many_lines)]
async fn main() -> anyhow::Result<()> {
    use clap::Parser;
    use hashbrown::{HashMap, HashSet};

    pretty_env_logger::init_timed();
    uscr::db::init_db().await;

    let args = Args::parse();
    std::fs::create_dir_all(&args.session)?;

    let clients = telegram::client::init_clients_from_map(
        telegram::parse_config(&args.config)?,
        args.session,
        args.flood_sleep_threshold,
    )
    .await;

    match args.command {
        Commands::Ping {
            channels: raw_channels,
        } => {
            use unicase::UniCase;
            use uscr::db::get_connection;

            let mut conn = get_connection().await?;
            let stmt = {
                const SQL: &str = "select from telegram.invite where hash = $1 union select from telegram.channel where name = $1";
                conn.prepare_static(SQL.into()).await?
            };

            let mut channels = HashMap::with_capacity(raw_channels.len());

            let mut ids = HashSet::with_capacity(raw_channels.len());
            let mut name_or_hashes = HashSet::with_capacity(raw_channels.len());

            for raw_channel in raw_channels {
                if let Ok(id) = raw_channel.parse() {
                    ids.insert(id);
                } else if conn.query_opt(&stmt, &[&raw_channel]).await?.is_none() {
                    name_or_hashes.insert(UniCase::new(raw_channel));
                }
            }

            if !ids.is_empty() {
                if let Some((app_id, client)) = clients.iter().next() {
                    for mut channel in
                        telegram::fetch_channels_by_id(client, ids.into_iter()).await?
                    {
                        channel.app_id = *app_id;
                        let t = UniCase::new((*channel.name).to_owned());
                        name_or_hashes.remove(&t);
                        channels.insert(channel.id, channel);
                    }
                } else {
                    tracing::warn!("no app found, skipping id lookup");
                }
            }

            if !name_or_hashes.is_empty() {
                let z_channels = ping::work(
                    name_or_hashes
                        .into_iter()
                        .map(UniCase::into_inner)
                        .collect(),
                    clients.iter(),
                    &mut conn,
                )
                .await;
                for channel in z_channels {
                    channels.insert(channel.id, channel);
                }
            }

            tracing::info!("{channels:#?}");
            insert_channels(channels.into_values(), &mut conn).await?;
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
            let mut channels_by_id = HashMap::with_capacity(clients.len());
            for channel in channels {
                if clients.get(&channel.app_id).is_some() {
                    channels_by_id
                        .entry(channel.app_id)
                        .or_insert_with(Vec::new)
                        .push(channel);
                } else {
                    tracing::warn!(target: "telegram-before-fetch", "app_id {} not found", channel.app_id);
                }
            }
            let futs = clients.iter().filter_map(|(id, client)| {
                let channels = channels_by_id.remove(id)?;
                let id = *id;
                Some(async move {
                    let target = format!("telegram-fetch-message({id})");
                    for channel in channels {
                        telegram::fetch_content(client, &channel, limit, &target).await;
                    }
                })
            });
            futures_util::future::join_all(futs).await;
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

    for (id, client) in &clients {
        if let Err(e) = client.save() {
            tracing::error!(target: "client-shutdown(save)", id, ?e);
        }
    }

    Ok(())
}
