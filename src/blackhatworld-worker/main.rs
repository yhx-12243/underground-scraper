#![feature(fn_traits, let_chains, try_blocks, unboxed_closures)]

mod browser;
mod worker;

const PROXY_HOST: Option<&str> = option_env!("PROXY_HOST");
const PROXY_USERNAME: Option<&str> = option_env!("PROXY_USERNAME");
const PROXY_PASSWORD: Option<&str> = option_env!("PROXY_PASSWORD");

#[derive(clap::Parser)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    Config {
        port: u16,
    },
    Work {
        #[arg(value_name = "file")]
        config: std::path::PathBuf,
        #[arg(short, long, default_value_t = 18322)]
        port: u16,
    },
}

#[derive(Hash, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct ConfigHeaders {
    #[serde(rename = "Cookie")]
    cookie: String,
    #[serde(rename = "User-Agent")]
    user_agent: String,
}

type WorkConfig = hashbrown::HashMap<compact_str::CompactString, ConfigHeaders>;

#[tokio::main]
#[allow(clippy::significant_drop_tightening)]
async fn main() -> anyhow::Result<()> {
    use clap::Parser;
    use worker::Worker;

    pretty_env_logger::init_timed();

    let args = Args::parse();

    let client = uscr::scrape::basic()?;

    match args.command {
        Commands::Config { port } => {
            let browser = uscr::scrape::puppeteer(
                false,
                PROXY_HOST.map(|host| format!("http://{host}:{port}")),
            )?;

            let tab = {
                let tabs_guard = browser
                    .get_tabs()
                    .lock()
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                let (first, remains) = tabs_guard
                    .split_first()
                    .ok_or_else(|| anyhow::anyhow!("no tabs found"))?;

                for remain in remains {
                    remain.close(true)?;
                }

                first.clone()
            };

            let user_agent = {
                use rand::seq::SliceRandom;
                let mut thread_rng = rand::thread_rng();
                *uscr::scrape::USER_AGENTS
                    .choose(&mut thread_rng)
                    .ok_or_else(|| anyhow::anyhow!("no UA available"))?
            };
            tracing::info!("choosing user-agent \x1b[1;36m{user_agent}\x1b[0m ...");

            tab.set_user_agent(user_agent, None, None)?;
            tab.enable_fetch(None, Some(true))?
                .authenticate(
                    PROXY_USERNAME.map(ToOwned::to_owned),
                    PROXY_PASSWORD.map(ToOwned::to_owned),
                )?
                .navigate_to("https://www.blackhatworld.com/")?;

            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

            let browser = browser::Browser::new(tab, tx, user_agent);
            tokio::task::spawn_blocking(move || browser.into_work());

            let mut set = hashbrown::HashSet::new();

            while let Some(headers) = rx.recv().await {
                use hashbrown::hash_set::Entry;
                if let Entry::Vacant(e) = set.entry(headers) {
                    tracing::info!(
                        "candidate:\n\x1b[1;35m\"{port}\": {}\x1b[0m",
                        serde_json::to_string_pretty(e.get()).unwrap_or_default()
                    );
                    e.insert();
                }
            }
        }
        Commands::Work {
            config,
            port: server_port,
        } => {
            let file = std::fs::File::open(config)?;
            let reader = std::io::BufReader::new(file);
            let config = serde_json::from_reader::<_, WorkConfig>(reader)?;

            let workers = config.into_iter().filter_map(|(port, headers)| {
                let client_port = port.parse().ok()?;
                (!(headers.cookie.is_empty() || headers.user_agent.is_empty())).then(|| Worker {
                    client_port,
                    server_port,
                    headers,
                    gateway: client.clone(),
                })
            });

            let futs = workers.map(Worker::into_future);
            futures_util::future::join_all(futs).await;
        }
    }

    Ok(())
}
