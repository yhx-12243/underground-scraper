#![feature(iter_next_chunk, try_blocks)]

mod scrape;

#[rustfmt::skip]
const CONFIGS: [(&str, i64); 27] = [
    ("bhw-marketplace-rules-and-how-to-post", 203),
    ("affiliate-programs-cpa-networks", 193),
    ("content-copywriting", 194),
    ("domains-websites-for-sale", 195),
    ("hosting", 196),
    ("hot-deals", 197),
    ("images-logos-videos", 198),
    ("misc", 18),
    ("proxies-for-sale", 112),
    ("seo-link-building", 43),
    ("seo-other", 199),
    ("seo-packages", 206),
    ("social-media", 200),
    ("social-media-panels", 302),
    ("web-design", 201),

    ("general-social-chat", 32),
    ("facebook", 86),
    ("instagram", 215),
    ("linkedin", 214),
    ("myspace", 87),
    ("pinterest", 211),
    ("reddit", 301),
    ("tiktok", 279),
    ("tumblr", 217),
    ("weibo", 216),
    ("x-formerly-twitter", 210),
    ("youtube", 77),
];

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pretty_env_logger::init_timed();
    uscr::db::init_db().await;

    let driver = uscr::scrape::get_driver(false).await?;

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
        let mut page = 1;
        loop {
            let r = scrape::work(page, &ctx).await;
            tokio::time::sleep(const { core::time::Duration::from_millis(2000) }).await;
            if r.is_break() {
                break;
            }
            page += 1;
        }
    }

    ctx.driver.close().await.map_err(Into::into)
}
