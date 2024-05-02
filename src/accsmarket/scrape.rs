use std::{
    hash::{BuildHasher, BuildHasherDefault, DefaultHasher},
    time::SystemTime,
};

use reqwest::{header::DATE, Client as Request};
use scraper::{Html, Selector};
use uscr::db::{get_connection, BB8Error, ToSqlIter};

pub struct Context {
    pub client: Request,
    pub sel_scp: Selector,
}

pub async fn work(id: i64, c_desc: String, ctx: &Context) {
    tracing::info!(target: "worker", "id = {id}, desc = {c_desc:?}");

    let form = [
        ("section", "get_soc"),
        ("cat_id", &id.to_string()),
        ("sort", "byPrice"),
    ];

    let request = ctx
        .client
        .post("https://accsmarket.com/req/soc.php")
        .form(&form);

    let res: Result<(String, SystemTime), reqwest::Error> = try {
        let res = request.send().await?;
        let Some(date) = res
            .headers()
            .get(DATE)
            .and_then(|s| s.to_str().ok())
            .and_then(|s| httpdate::parse_http_date(s).ok())
        else {
            tracing::warn!(target: "worker", "[#{id}] no/wrong date");
            return;
        };
        (res.text().await?, date)
    };
    let (res, date) = match res {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(target: "worker", "[#{id}] err: {e:?}");
            return;
        }
    };

    let fragment = Html::parse_fragment(&res);
    let root = fragment.root_element();
    let mut archived = Vec::new();
    for child in root.child_elements() {
        let quantity = child
            .attr("data-qty")
            .and_then(|x| x.parse().ok())
            .unwrap_or(0i64);
        let cost = child
            .attr("data-cost")
            .and_then(|x| x.replace(',', ".").parse().ok())
            .unwrap_or(0.0f64);
        let desc = if let Some(d) = child.select(&ctx.sel_scp).next() {
            d.text().map(str::trim).collect()
        } else {
            String::new()
        };

        let hash = BuildHasherDefault::<DefaultHasher>::default().hash_one(&desc);

        archived.push((hash as i64, desc, quantity, cost));
    }

    if !archived.is_empty() {
        let res: Result<(), BB8Error> = try {
            const SQL: &str = "with tmp_insert(i, d, q, p) as (select * from unnest($1::bigint[], $4::text[], $5::bigint[], $6::float8[])) insert into accs.market (id, category, time, description, quantity, price) select i, $2, $3, d, q, p from tmp_insert";

            let mut conn = get_connection().await?;
            let stmt = conn.prepare_static(SQL.into()).await?;
            conn.execute(
                &stmt,
                &[
                    &ToSqlIter(archived.iter().map(|x| x.0)),
                    &id,
                    &date,
                    &ToSqlIter(archived.iter().map(|x| &*x.1)),
                    &ToSqlIter(archived.iter().map(|x| x.2)),
                    &ToSqlIter(archived.iter().map(|x| x.3)),
                ],
            )
            .await?;

            tracing::info!(target: "worker", "\x1b[36m[#{id}] update {} items\x1b[0m", archived.len());
        };
        if let Err(e) = res {
            tracing::error!(target: "worker", "\x1b[31m[#{id}] {e:?}\x1b[0m");
        }
    }
}
