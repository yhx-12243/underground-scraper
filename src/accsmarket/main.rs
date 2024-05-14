#![feature(try_blocks)]

mod scrape;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pretty_env_logger::init_timed();
    uscr::db::init_db().await;

    let client = uscr::scrape::basic()?;

    let res = client
        .get("https://accsmarket.com/")
        .send()
        .await?
        .text()
        .await?;
    let html = scraper::Html::parse_document(&res);

    let container = html
        .select(&scraper::Selector::parse(".soc-bl").unwrap())
        .next()
        .ok_or_else(|| anyhow::anyhow!("element not found"))?;

    let ctx = scrape::Context {
        client,
        sel_scp: scraper::Selector::parse(".soc-text>p").unwrap(),
    };

    let sel_h2 = scraper::Selector::parse("h2").unwrap();
    let mut id = 0;
    let mut desc = String::new();
    let mut futs = Vec::new();
    for child in container.child_elements() {
        match child.attr("class") {
            Some("soc-title") => {
                if let Some(h2) = child.select(&sel_h2).next() {
                    id = h2.attr("data-id").and_then(|x| x.parse().ok()).unwrap_or(0);
                    desc = h2.text().map(str::trim).collect();
                }
            }
            Some("socs") => {
                futs.push(scrape::work(id, core::mem::take(&mut desc), &ctx));
            }
            e => tracing::warn!(target: "soc-bl", "Unknown class: {e:?}"),
        }
    }

    for fut in futs {
        fut.await;
        tokio::time::sleep(const { core::time::Duration::from_millis(250) }).await;
    }
    // futures_util::future::join_all(futs).await;

    Ok(())
}
