#![feature(
    future_join,
    let_chains,
    stmt_expr_attributes,
    try_blocks,
    write_all_vectored,
    yeet_expr,
)]

mod db;
mod extract;
mod ping;
mod telegram;

#[derive(clap::Parser)]
struct Args {
    #[command(subcommand)]
    command: Commands,
    #[arg(
        short,
        long,
        default_value = "./telegram/sessions",
        value_name = "dir",
        help = "The directory that stores sessions"
    )]
    session: std::path::PathBuf,
    #[arg(
        short,
        long,
        default_value = "./telegram/config.json",
        value_name = "file",
        help = "The config file"
    )]
    config: std::path::PathBuf,
    #[arg(
        long,
        default_value_t = 192,
        value_name = "seconds",
        help = "Flood sleep threshold"
    )]
    flood_sleep_threshold: u32,
}

#[derive(clap::Subcommand)]
enum Commands {
    Ping {
        #[arg(short, long, num_args = 1.., required = true)]
        channels: Vec<compact_str::CompactString>,
        #[arg(short, long)]
        force: bool,
    },
    Content {
        #[arg(short, long, num_args = 1.., value_parser = clap::value_parser!(i64).range(0..))]
        channels: Vec<i64>,
        #[arg(short, long, default_value_t = 10240)]
        limit: u32,
    },
    Extract {
        #[arg(short, long, default_value = "ids.txt")]
        save: std::path::PathBuf,
    },
    Interact {
        #[arg(short, long, num_args = 1.., required = true, value_parser = clap::value_parser!(i64).range(0..))]
        peers: Vec<i64>,
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

    let mut clients = telegram::client::init_clients_from_map(
        telegram::parse_config(&args.config)?,
        args.session,
        args.flood_sleep_threshold,
    ).await;

    let mut conn = uscr::db::get_connection().await?;

    match args.command {
        Commands::Ping { channels: raw_channels, force } => {
            let searched = if force {
                HashMap::default()
            } else {
                db::get_searched_peers(&mut conn).await?
            };

            let mut channels = HashMap::with_capacity(raw_channels.len());
            let mut users = HashMap::with_capacity(raw_channels.len());

            let (ids, mut name_or_hashes) = ping::separate_id_and_names(raw_channels, &searched);

            if !ids.is_empty() {
                if let Some((app_id, client)) = clients.iter().next() {
                    for mut channel in telegram::fetch_channels_by_id(client, ids.into_iter()).await? {
                        channel.app_id = *app_id;
                        let t = unicase::UniCase::new(channel.name);
                        name_or_hashes.remove(&t);
                        channel.name = t.into_inner();
                        channels.insert(channel.id, channel);
                    }
                } else {
                    tracing::warn!("no app found, skipping id lookup");
                }
            }

            if !name_or_hashes.is_empty() {
                let (z_channels, z_users) = ping::work(
                    name_or_hashes.into_iter().map(unicase::UniCase::into_inner).collect(),
                    clients.iter(),
                    &mut conn,
                ).await;
                for channel in z_channels {
                    channels.insert(channel.id, channel);
                }
                for user in z_users {
                    users.insert(user.peer.id, user);
                }
            }

            tracing::info!("{channels:#?}");
            db::insert_channels(channels.into_values(), &mut conn).await?;

            tracing::info!("{users:#?}");
            db::insert_users(users.into_values().filter(telegram::User::maybe_bot), &mut conn).await?;
        }
        Commands::Content { channels: channels_filt, limit } => {
            let mut channels = db::get_all_channels_from_db(&mut conn).await?;
            if !channels_filt.is_empty() {
                let filt = channels_filt.into_iter().collect::<HashSet<i64>>();
                channels.retain(|channel| filt.contains(&channel.id));
            }
            let mut channels_by_id = HashMap::with_capacity(clients.len());
            for channel in channels {
                if clients.contains_key(&channel.app_id) {
                    channels_by_id
                        .entry(channel.app_id)
                        .or_insert_with(Vec::new)
                        .push(channel);
                } else {
                    tracing::warn!(target: "telegram-before-fetch", "app_id {} not found", channel.app_id);
                }
            }

            let stmt_get_range;
            let stmt_insert_msg;
            let stmt_upd_minmax;
            let db = {
                stmt_get_range = conn.prepare_static("select min_message_id, max_message_id from telegram.channel where id = $1".into()).await?;
                stmt_insert_msg = conn.prepare_static("with tmp_insert(m, d) as (select * from unnest($1::integer[], $3::jsonb[])) insert into telegram.message (id, message_id, channel_id, data) select ($2::bigint << 32) | m, m, $2, d from tmp_insert on conflict (id) do update set message_id = excluded.message_id, channel_id = excluded.channel_id, data = excluded.data returning xmax".into()).await?;
                stmt_upd_minmax = conn.prepare_static("update telegram.channel set min_message_id = $1, max_message_id = $2, last_fetch = now() at time zone 'UTC' where id = $3".into()).await?;

                db::DBWrapper {
                    conn: &conn,
                    stmts: [&stmt_get_range, &stmt_insert_msg, &stmt_upd_minmax],
                }
            };
            let futs = clients.iter().filter_map(|(id, client)| {
                let channels = channels_by_id.remove(id)?;
                let id = *id;
                Some(async move {
                    let target = format!("telegram-fetch-message({id})");
                    for channel in channels {
                        telegram::fetch_content(client, &channel, limit, &target, db).await;
                    }
                })
            });
            futures_util::future::join_all(futs).await;
        }
        Commands::Extract { save } => {
            let map = db::get_searched_peers(&mut conn).await?;

            let mut inspector = extract::Inspector::new(
                map,
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(save)?,
            );
            inspector.extract_content(&mut conn).await?;
        }
        Commands::Interact { peers: peers_filt } => {
            let mut peers = db::get_all_bots_from_db(&mut conn).await?;
            {
                let filt = peers_filt.into_iter().collect::<HashSet<i64>>();
                peers.retain(|peer| filt.contains(&peer.peer.id));
            }
            let mut peers_by_id = HashMap::with_capacity(clients.len());
            for peer in peers {
                if clients.contains_key(&peer.peer.app_id) {
                    peers_by_id
                        .entry(peer.peer.app_id)
                        .or_insert_with(Vec::new)
                        .push(peer);
                } else {
                    tracing::warn!(target: "telegram-before-interact", "app_id {} not found", peer.peer.app_id);
                }
            }

            let stmt = conn.prepare_static("insert into telegram.interaction (bot_id, message_id, request, response) values ($1, $2, $3, $4) on conflict (bot_id, message_id) do update set request = excluded.request, response = excluded.response".into()).await?;
            let db = db::DBWrapper {
                conn: &conn,
                stmts: [&stmt],
            };
            let futs = clients.iter_mut().filter_map(|(id, client)|
                Some(telegram::interact_bot_into_future(
                    client,
                    peers_by_id.remove(id)?,
                    format!("telegram-interact-bot({id})"),
                    db,
                ))
            );
            futures_util::future::join_all(futs).await;
        }
    }

    for (id, client) in &clients {
        if let Err(e) = client.save() {
            tracing::error!(target: "client-shutdown(save)", id, ?e);
        }
    }

    Ok(())
}
