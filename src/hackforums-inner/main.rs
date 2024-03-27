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
    iter_next_chunk,
    lazy_cell,
    stmt_expr_attributes,
    try_blocks,
    yeet_expr
)]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pretty_env_logger::init_timed();
    t2::db::init_db().await;

    let ids: Vec<i64> = {
        const SQL: &str = "select distinct id from hackforums.posts natural left outer join hackforums.content where hackforums.content.id is null order by id";
        let mut conn = t2::db::get_connection().await?;
        let stmt = conn.prepare_static(SQL.into()).await?;
        conn.query(&stmt, &[])
            .await?
            .into_iter()
            .filter_map(|row| row.try_get(0).ok())
            .collect()
    };

    dbg!(ids);

    Ok(())
}

/*
const dp = new DOMParser();

async function work(id) {
    const txt = await fetch(`https://hackforums.net/showthread.php?tid=${id}`).then(x => x.text());
    const doc = dp.parseFromString(txt, 'text/html');
    const div = doc.querySelector('.post_content');
    const [head, body] = div.children;
    const date_str = head.querySelector('.post_date').firstChild.textContent;
    const date = new Date(`${date_str}Z`);
    const content = body.textContent;
    return [date, content];
}
*/
