#![feature(ascii_char, iter_next_chunk, try_blocks)]

mod scrape;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pretty_env_logger::init_timed();
    uscr::db::init_db().await;

    let driver = uscr::scrape::get_driver(false).await?;

    let ctx = scrape::Context {
        driver,
        sel_content_tr: scraper::Selector::parse("tr").unwrap(),
        sel_subject_old: scraper::Selector::parse(".subject_old,.subject_new").unwrap(),
    };

    for i in 1..=2631 {
        scrape::work(i, &ctx).await;
        tokio::time::sleep(const { core::time::Duration::from_millis(250) }).await;
    }

    ctx.driver.close().await.map_err(Into::into)
}
