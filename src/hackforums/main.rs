#![feature(
    allocator_api,
    inline_const,
    iter_array_chunks,
    iter_next_chunk,
    lazy_cell,
    stmt_expr_attributes,
    try_blocks,
    yeet_expr,
)]

mod scrape;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pretty_env_logger::init_timed();
    t2::db::init_db().await;

    let driver = t2::scrape::get_driver(false).await?;

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
