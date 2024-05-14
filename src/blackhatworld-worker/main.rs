#![feature(let_chains, try_blocks)]

mod worker;

use compact_str::CompactString;
use hashbrown::HashMap;
use serde::Deserialize;

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

#[derive(Deserialize)]
struct ConfigHeaders {
    #[serde(rename = "Cookie")]
    cookie: String,
    #[serde(rename = "User-Agent")]
    user_agent: String,
}

type WorkConfig = HashMap<CompactString, ConfigHeaders>;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use clap::Parser;
    use worker::Worker;

    pretty_env_logger::init_timed();

    let args = Args::parse();

    let client = reqwest::Client::builder()
        .connect_timeout(const { core::time::Duration::from_secs(5) })
        .build()?;

    match args.command {
        Commands::Config { port } => {
            dbg!(port);
        }
        Commands::Work {
            config,
            port: server_port,
        } => {
            let file = std::fs::File::open(config)?;
            let reader = std::io::BufReader::new(file);
            let config = serde_json::from_reader::<_, WorkConfig>(reader)?;

            let workers = config
                .into_iter()
                .filter_map(|(port, headers)| {
                    let port = port.parse::<u16>().ok()?;
                    (!(headers.cookie.is_empty() || headers.user_agent.is_empty())).then(|| {
                        Worker {
                            client_port: port,
                            server_port,
                            headers,
                            gateway: client.clone(),
                        }
                    })
                })
                .collect::<Vec<_>>();

            let futs = workers.into_iter().map(Worker::into_future);
            futures_util::future::join_all(futs).await;
        }
    }

    Ok(())
}
