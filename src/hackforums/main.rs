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

    let driver = t2::scrape::get_driver(false).await?;

    let ctx = scrape::Context {
        driver,
        sel_content_tr: scraper::Selector::parse("tr").unwrap(),
        sel_subject_old: scraper::Selector::parse(".subject_old,.subject_new").unwrap(),
    };

    let mut res = Vec::new();
    for i in 1..=558 {
        let mut block = scrape::work(i, &ctx).await;
        res.append(&mut block);
        tokio::time::sleep(const { core::time::Duration::from_millis(500) }).await;
    }

    if !res.is_empty() {
        use t2::db::{get_connection, BB8Error, ToSqlIter};

        let res: Result<(), BB8Error> = try {
            const SQL: &str = "with tmp_insert(i, t, r, v, l) as (select * from unnest($1::bigint[], $2::text[], $3::bigint[], $4::bigint[], $5::timestamp[])) insert into hackforums.posts (id, title, replies, views, lastpost, time) select i, t, r, v, l, now() at time zone 'UTC' from tmp_insert";

            let mut conn = get_connection().await?;
            let stmt = conn.prepare_static(SQL.into()).await?;
            conn.execute(
                &stmt,
                &[
                    &ToSqlIter(res.iter().map(|x| x.tid)),
                    &ToSqlIter(res.iter().map(|x| &*x.title)),
                    &ToSqlIter(res.iter().map(|x| x.replies)),
                    &ToSqlIter(res.iter().map(|x| x.views)),
                    &ToSqlIter(res.iter().map(|x| x.lastPost)),
                ],
            )
            .await?;

            tracing::info!(target: "db", "\x1b[36mupdate {} items\x1b[0m", res.len());
        };
        if let Err(e) = res {
            tracing::error!(target: "db", "\x1b[31mdb err: {e}\x1b[0m");
        }
    }

    ctx.driver.close().await.map_err(Into::into)
}
