#![feature(iter_next_chunk, try_blocks)]

mod scrape;

#[rustfmt::skip]
const CONFIGS: [(&str, i64); 65] = [
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

    ("ai-artificial-intelligence-in-digital-marketing", 252),
    ("black-hat-seo", 28),
    ("black-hat-seo-tools", 9),
    ("blogging", 3),
    ("cloaking-and-content-generators", 2),
    ("proxies", 101),
    ("voice-search", 280),

    ("copywriting-sales-persuasion", 168),
    ("domain-names-parking", 53),
    ("graphic-design", 169),
    ("link-building", 108),
    ("local-seo", 209),
    ("video-production", 170),
    ("web-hosting", 94),
    ("white-hat-seo", 30),

    ("associated-content-writing-articles", 107),
    ("affiliate-programs", 15),
    ("business-tax-advice", 96),
    ("cpa", 50),
    ("cryptocurrency", 218),
    ("dropshipping-wholesale-hookups", 68),
    ("ebay", 69),
    ("hire-a-freelancer", 76),
    ("joint-ventures", 65),
    ("making-money", 12),
    ("media-buying", 175),
    ("membership-sites", 106),
    ("mobile-marketing", 158),
    ("my-journey-discussions", 167),
    ("new-markets", 208),
    ("offline-marketing", 132),
    ("pay-per-click", 13),
    ("pay-per-install", 205),
    ("pay-per-view", 102),
    ("site-flipping", 141),
    ("torrents", 75),
    ("freebies-giveaways", 174),
    ("service-reviews-beta-testers-help-wanted", 165),
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
