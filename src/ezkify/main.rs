#![feature(iter_array_chunks, iter_next_chunk)]

mod parse_item;

#[derive(clap::Parser)]
struct Args {
    #[arg(short, long, default_value = "ezkify")]
    key: compact_str::CompactString,
    #[arg(short, long)]
    url: Option<String>,
}

async fn insert_category(
    conn: &mut tokio_postgres::Client,
    key: &str,
    cid: i64,
    desc: &str,
) -> uscr::db::DBResult<()> {
    const SQL: &str = "insert into ezkify.categories (key, id, \"desc\") values ($1, $2, $3) on conflict (key, id) do update set \"desc\" = excluded.desc";
    let stmt = conn.prepare_static(SQL.into()).await?;
    conn.execute(&stmt, &[&key, &cid, &desc]).await?;
    Ok(())
}

struct Item {
    id: i64,
    time: std::time::SystemTime,
    cid: i64,
    service: String,
    rate_per_1k: f64,
    min_order: i64,
    max_order: i64,
    description: String,
}

#[allow(clippy::too_many_arguments)]
async fn insert_db(
    conn: &mut tokio_postgres::Client,
    key: &str,
    item: Item,
) -> uscr::db::DBResult<()> {
    const SQL: &str = "insert into ezkify.items (key, id, time, category_id, service, rate_per_1k, min_order, max_order, description) values ($1, $2, $3, $4, $5, $6, $7, $8, $9)";
    let stmt = conn.prepare_static(SQL.into()).await?;
    conn.execute(&stmt, &[
        &key,
        &item.id,
        &item.time,
        &item.cid,
        &item.service,
        &item.rate_per_1k,
        &item.min_order,
        &item.max_order,
        &item.description,
    ])
    .await?;
    Ok(())
}

struct ScrapeConfig {
    category_class: &'static str,
    parse_item: fn(row: scraper::ElementRef) -> anyhow::Result<Item>,
    table_selector: scraper::Selector,
}

fn get_config(key: &str) -> ScrapeConfig {
    let category_class = match key {
        "smmrapid" => "category",
        "dripfeedpanel" => "cat-name servicescategory",
        _ => "services-list-category-title",
    };

    let parse_item = match key {
        // "smmrapid" => parse_item::smmrapid::parse,
        "dripfeedpanel" => parse_item::dripfeedpanel::parse,
        _ => parse_item::ezkify::parse,
    };

    let table_selector_s = match key {
        "smmrapid" => ".services>.container-xxl",
        "dripfeedpanel" => "#service-table>tbody",
        _ => "#service-tbody",
    };

    ScrapeConfig {
        category_class,
        parse_item,
        table_selector: scraper::Selector::parse(table_selector_s).unwrap(),
    }
}

#[tokio::main]
#[allow(clippy::too_many_lines)]
async fn main() -> anyhow::Result<()> {
    use clap::Parser;
    use parse_item::{CID, GLOBAL_DATE};
    use std::sync::atomic::Ordering;

    pretty_env_logger::init_timed();
    uscr::db::init_db().await;

    let args = Args::parse();
    let client = uscr::scrape::simple();

    tracing::info!(target: "main", "start fetching ...");
    let res = client
        .get(args.url.unwrap_or_else(|| format!("https://{}.com/services", args.key)))
        .send()
        .await?;

    *GLOBAL_DATE.write() = httpdate::parse_http_date(
        res.headers()
            .get(reqwest::header::DATE)
            .ok_or_else(|| anyhow::anyhow!("no date"))?
            .to_str()?,
    )?;

    let res = res.text().await?;
    tracing::info!(target: "main", "fetching finished: {} bytes", res.len());
    let html = scraper::Html::parse_document(&res);
    tracing::info!(target: "main", "parsing finished.");

    let config = get_config(&args.key);
    let mut conn = uscr::db::get_connection().await?;

    let tbody = html
        .select(&config.table_selector)
        .next()
        .ok_or_else(|| anyhow::anyhow!("element not found"))?;

    if args.key == "smmrapid" {
        for category in tbody.child_elements() {
            use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};

            if category.attr("class") != Some(config.category_class) {
                continue;
            }
            let [category, items] = category
                .child_elements()
                .next_chunk()
                .map_err(|e| anyhow::anyhow!("category error: {e:?}"))?;
            let desc = category.text().map(str::trim).collect::<String>();
            let cid = BuildHasherDefault::<DefaultHasher>::default()
                .hash_one(&desc)
                .cast_signed();

            insert_category(&mut conn, &args.key, cid, &desc).await?;
            CID.store(cid, Ordering::SeqCst);

            for [item, modal] in items.child_elements().array_chunks() {
                match parse_item::smmrapid::parse(item, modal) {
                    Ok(item) => insert_db(&mut conn, &args.key, item).await?,
                    Err(e) => tracing::error!(?e),
                }
            }
        }
    } else {
        for row in tbody.child_elements() {
            if row.attr("class") == Some(config.category_class) {
                let cid = row
                    .attr("data-filter-table-category-id")
                    .and_then(|x| x.parse().ok())
                    .unwrap_or(-1);
                let desc = row.text().map(str::trim).collect::<String>();
                insert_category(&mut conn, &args.key, cid, &desc).await?;
                CID.store(cid, Ordering::SeqCst);
            } else {
                match (config.parse_item)(row) {
                    Ok(item) => insert_db(&mut conn, &args.key, item).await?,
                    Err(e) => tracing::error!(?e),
                }
            }
        }
    }

    Ok(())
}

/*

bin/ezkify -k ezkify
bin/ezkify -k smmrapid
bin/ezkify -k dripfeedpanel
bin/ezkify -k fullsmm -u https://panel.fullsmm.com/services
bin/ezkify -k smmcost
bin/ezkify -k n1panel

*/
