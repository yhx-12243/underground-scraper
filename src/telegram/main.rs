#![feature(const_int_from_str)]

mod telegram;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pretty_env_logger::init_timed();
    uscr::db::init_db().await;

    let client = telegram::get_client().await?;
    telegram::login(&client).await?;
    telegram::save(&client)?;

    let result = telegram::get_channel_info(&client, 1_491_178_054).await?;

    tracing::info!("result: {result:#?}");

    Ok(())
}
