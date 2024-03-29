#![warn(clippy::pedantic, clippy::nursery)]
#![allow(
    clippy::absolute_paths,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_lossless, // u32 -> u64
    clippy::cast_possible_truncation, // u64 -> u32
    clippy::cast_possible_wrap, // u32 -> i32
    clippy::cast_sign_loss, // i32 -> u32
    clippy::option_if_let_else,
    clippy::future_not_send,
    clippy::host_endian_bytes,
    clippy::implicit_return,
    clippy::indexing_slicing,
    clippy::inline_always,
    clippy::integer_division,
    clippy::min_ident_chars,
    clippy::missing_assert_message,
    clippy::missing_trait_methods,
    clippy::module_name_repetitions,
    clippy::multiple_unsafe_ops_per_block,
    clippy::needless_pass_by_value,
    clippy::non_ascii_literal,
    clippy::single_char_lifetime_names,
    clippy::pattern_type_mismatch,
    clippy::pub_use,
    clippy::question_mark_used,
    clippy::ref_patterns,
    clippy::self_named_module_files,
    clippy::shadow_reuse,
    clippy::shadow_unrelated,
    clippy::similar_names,
    clippy::single_call_fn,
    clippy::std_instead_of_alloc,
    clippy::std_instead_of_core,
    clippy::string_add,
    clippy::unseparated_literal_suffix,
    clippy::wildcard_enum_match_arm,
    internal_features,
    non_snake_case,
)]
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

use t2::db::get_connection;

async fn insert_category(
    conn: &mut tokio_postgres::Client,
    cid: i64,
    desc: &str,
) -> t2::db::DBResult<()> {
    const SQL: &str = "insert into ezkify.categories (id, \"desc\") values ($1, $2) on conflict (id) do update set \"desc\" = excluded.desc";
    let stmt = conn.prepare_static(SQL.into()).await?;
    conn.execute(&stmt, &[&cid, &desc]).await?;
    Ok(())
}

async fn insert_db(
    conn: &mut tokio_postgres::Client,
    id: i64,
    time: std::time::SystemTime,
    cid: i64,
    service: &str,
    rate_per_1k: f64,
    min_order: i64,
    max_order: i64,
    description: &str,
) -> t2::db::DBResult<()> {
    const SQL: &str = "insert into ezkify.items (id, time, category_id, service, rate_per_1k, min_order, max_order, description) values ($1, $2, $3, $4, $5, $6, $7, $8)";
    let stmt = conn.prepare_static(SQL.into()).await?;
    conn.execute(
        &stmt,
        &[
            &id,
            &time,
            &cid,
            &service,
            &rate_per_1k,
            &min_order,
            &max_order,
            &description,
        ],
    )
    .await?;
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pretty_env_logger::init_timed();
    t2::db::init_db().await;

    let client = reqwest::Client::builder()
        .connect_timeout(const { core::time::Duration::from_secs(5) })
        .build()?;

    tracing::info!(target: "main", "start fetching ...");
    let res = client.get("https://ezkify.com/services").send().await?;

    let date = httpdate::parse_http_date(
        res.headers()
            .get(reqwest::header::DATE)
            .ok_or_else(|| anyhow::anyhow!("no date"))?
            .to_str()?,
    )?;

    let res = res.text().await?;
    tracing::info!(target: "main", "fetching finished: {} bytes", res.len());
    let html = scraper::Html::parse_document(&res);
    tracing::info!(target: "main", "parsing finished.");

    let tbody = html
        .select(&scraper::Selector::parse("#service-tbody").unwrap())
        .next()
        .ok_or_else(|| anyhow::anyhow!("element not found"))?;

    let sel_dnone = scraper::Selector::parse(".d-none").unwrap();
    let mut cid = 0;
    let mut conn = get_connection().await?;
    for row in tbody.child_elements() {
        if row.attr("class") == Some("services-list-category-title") {
            cid = row
                .attr("data-filter-table-category-id")
                .and_then(|x| x.parse().ok())
                .unwrap_or(-1);
            let desc = row.text().map(str::trim).collect::<String>();
            insert_category(&mut conn, cid, &desc).await?;
        } else {
            let cells = row
                .child_elements()
                .next_chunk::<6>()
                .map_err(|e| anyhow::anyhow!("child error: {e:?}"))?;

            let Some(id) = cells[0]
                .attr("data-filter-table-service-id")
                .and_then(|x| x.parse::<i64>().ok())
            else {
                tracing::warn!(target: "main", "id error: {}", cells[0].html());
                continue;
            };
            let service = cells[1].text().map(str::trim).collect::<String>();
            let Some(rate_per_1k) = cells[2]
                .text()
                .map(str::trim)
                .collect::<String>()
                .strip_prefix('$')
                .and_then(|x| x.parse::<f64>().ok())
            else {
                tracing::warn!(target: "main", "rate error: {}", cells[1].html());
                continue;
            };
            let Ok(min_order) = cells[3]
                .text()
                .map(|c| c.replace(char::is_whitespace, ""))
                .collect::<String>()
                .parse::<i64>()
            else {
                tracing::warn!(target: "main", "min_order error: {}", cells[3].html());
                continue;
            };
            let Ok(max_order) = cells[4]
                .text()
                .map(|c| c.replace(char::is_whitespace, ""))
                .collect::<String>()
                .parse::<i64>()
            else {
                tracing::warn!(target: "main", "max_order error: {}", cells[4].html());
                continue;
            };

            let description = if let Some(dnone) = cells[5].select(&sel_dnone).next() {
                let mut s = String::new();
                for node in dnone.children() {
                    match node.value() {
                        scraper::Node::Text(text) => s.push_str(text),
                        scraper::Node::Element(elem) if elem.name() == "br" => s.push('\n'),
                        _ => (),
                    }
                }
                s
            } else {
                String::new()
            };

            insert_db(
                &mut conn,
                id,
                date,
                cid,
                &service,
                rate_per_1k,
                min_order,
                max_order,
                &description,
            )
            .await?;
        }
    }

    Ok(())
}
