#![feature(iter_next_chunk, try_blocks)]

mod scrape;

#[rustfmt::skip]
const CONFIGS: [(&str, i64, i32); 13] = [
    ("bhw-marketplace-rules-and-how-to-post", 203, 1),
    ("affiliate-programs-cpa-networks", 193, 15),
    // ("content-copywriting", 194),
    ("domains-websites-for-sale", 195, 19),
    ("hosting", 196, 14),
    ("hot-deals", 197, 11),
    ("images-logos-videos", 198, 13),
    ("misc", 18, 120),
    ("proxies-for-sale", 112, 119),
    ("seo-link-building", 43, 241),
    ("seo-other", 199, 12),
    ("seo-packages", 206, 32),
    // ("social-media", 200),
    ("social-media-panels", 302, 8),
    ("web-design", 201, 25),

    // ("general-social-chat", 32, ?),
    // ("facebook", 86, ?),
    // ("instagram", 215, ?),
    // ("linkedin", 214, ?),
    // ("myspace", 87, ?),
    // ("pinterest", 211, ?),
    // ("reddit", 301, ?),
    // ("tiktok", 279, ?),
    // ("tumblr", 217, ?),
    // ("weibo", 216, ?),
    // ("x-formerly-twitter", 210, ?),
    // ("youtube", 77, ?),
];

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pretty_env_logger::init_timed();
    t2::db::init_db().await;

    let driver = t2::scrape::get_driver(false).await?;

    let mut ctx = scrape::Context {
        driver,
        cfg: Default::default(),
        reg_id: regex::Regex::new(r"js-threadListItem-(\d+)").unwrap(),
        sel_struct_item: scraper::Selector::parse(".structItem").unwrap(),
        sel_title: scraper::Selector::parse(".structItem-title>a").unwrap(),
        sel_udt: scraper::Selector::parse("time.u-dt").unwrap(),
        sel_dd: scraper::Selector::parse("dd").unwrap(),
    };

    for config in CONFIGS {
        ctx.cfg = config;
        for i in 1..=config.2 {
            scrape::work(i, &ctx).await;
            tokio::time::sleep(const { core::time::Duration::from_millis(2000) }).await;
        }
    }

    ctx.driver.close().await.map_err(Into::into)
}
