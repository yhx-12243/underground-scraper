#![feature(iter_next_chunk, try_blocks)]

mod scrape;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pretty_env_logger::init_timed();
    t2::db::init_db().await;

    let driver = t2::scrape::get_driver(false).await?;

    let ctx = scrape::Context {
        driver,
        reg_id: regex::Regex::new(r"js-threadListItem-(\d+)").unwrap(),
        sel_struct_item: scraper::Selector::parse(".structItem").unwrap(),
        sel_title: scraper::Selector::parse(".structItem-title>a").unwrap(),
        sel_udt: scraper::Selector::parse("time.u-dt").unwrap(),
        sel_dd: scraper::Selector::parse("dd").unwrap(),
    };

    for i in 1..=1713 {
        scrape::work(i, &ctx).await;
        tokio::time::sleep(const { core::time::Duration::from_millis(2000) }).await;
    }

    ctx.driver.close().await.map_err(Into::into)
}
