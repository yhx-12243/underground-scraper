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

mod scrape;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pretty_env_logger::init_timed();
    t2::db::init_db().await;

    let client = reqwest::Client::builder()
        .connect_timeout(const { core::time::Duration::from_secs(5) })
        .build()?;

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
        tokio::time::sleep(core::time::Duration::from_millis(250)).await;
    }
    // futures_util::future::join_all(futs).await;

    Ok(())
}
