use std::{
    ops::ControlFlow,
    time::{Duration, SystemTime},
};

use fantoccini::{Client as Driver, Locator};
use regex::Regex;
use scraper::{Html, Selector};
use uscr::{
    db::{get_connection, BB8Error, ToSqlIter},
    util::xmax_to_success,
};

pub struct Context {
    pub driver: Driver,
    pub cfg: (&'static str, i64),
    pub reg_id: Regex,
    pub sel_struct_item: Selector,
    pub sel_title: Selector,
    pub sel_udt: Selector,
    pub sel_dd: Selector,
}

#[derive(Debug)]
pub struct Post {
    pub id: i64,
    pub author: String,
    pub title: String,
    pub time: SystemTime,
    pub replies: i64,
    pub views: i64,
    pub lastReply: SystemTime,
}

fn _pa(x: &str) -> Option<i64> {
    x.replace('K', "000").replace('M', "000000").parse().ok()
}

#[allow(clippy::too_many_lines)]
pub async fn work(page: i32, ctx: &Context) -> ControlFlow<(), ()> {
    tracing::info!(target: "worker", "[Forum \x1b[33m{}\x1b[0m] [Page \x1b[32m#{page}\x1b[0m] start", ctx.cfg.0);

    let url = format!(
        "https://www.blackhatworld.com/forums/{}.{}/page-{page}/?order=post_date&direction=desc",
        ctx.cfg.0, ctx.cfg.1,
    );

    if let Err(e) = ctx.driver.goto(&url).await {
        tracing::warn!(target: "worker", "[Page #{page}] err: {e:?}");
        return ControlFlow::Continue(());
    }

    let locator = Locator::Css(".js-threadList");
    if let Err(e) = ctx.driver.wait().forever().for_element(locator).await {
        tracing::warn!(target: "worker", "[Page #{page}] err: {e:?}");
        return ControlFlow::Continue(());
    }

    let list = match ctx.driver.find(Locator::Css(".structItemContainer")).await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(target: "worker", "[Page #{page}] err: {e:?}");
            return ControlFlow::Continue(());
        }
    };

    let html = match list.html(false).await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(target: "worker", "[Page #{page}] err: {e:?}");
            return ControlFlow::Continue(());
        }
    };

    let fragment = Html::parse_fragment(&html);
    let res = fragment
        .select(&ctx.sel_struct_item)
        .filter_map(|entry| {
            let c = entry.child_elements().next_chunk::<4>().ok()?;

            let id = ctx
                .reg_id
                .captures(entry.attr("class")?)?
                .get(1)?
                .as_str()
                .parse()
                .ok()?;
            let author = entry.attr("data-author")?.to_owned();
            let title = c[1]
                .select(&ctx.sel_title)
                .next()?
                .text()
                .map(str::trim)
                .collect();
            let time = SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(
                c[1].select(&ctx.sel_udt)
                    .next()?
                    .attr("data-time")?
                    .parse()
                    .ok()?,
            ))?;

            let mut dd = c[2].select(&ctx.sel_dd);
            let replies = _pa(&dd.next()?.text().map(str::trim).collect::<String>())?;
            let views = _pa(&dd.next()?.text().map(str::trim).collect::<String>())?;

            let lastReply = SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(
                c[3].select(&ctx.sel_udt)
                    .next()?
                    .attr("data-time")?
                    .parse()
                    .ok()?,
            ))?;

            Some(Post {
                id,
                author,
                title,
                time,
                replies,
                views,
                lastReply,
            })
        })
        .collect::<Vec<_>>();

    if !res.is_empty() {
        let res: Result<(), BB8Error> = try {
            const SQL: &str = "with tmp_insert(i, a, t, c, r, v, l) as (select * from unnest($2::bigint[], $3::text[], $4::text[], $5::timestamp[], $6::bigint[], $7::bigint[], $8::timestamp[])) insert into blackhatworld.posts (id, time, author, title, create_time, replies, views, last_reply, section) select i, now() at time zone 'UTC', a, t, c, r, v, l, $1 from tmp_insert on conflict (id) do update set time = excluded.time, author = excluded.author, title = excluded.title, replies = excluded.replies, views = excluded.views, last_reply = excluded.last_reply returning xmax";

            let mut conn = get_connection().await?;
            let stmt = conn.prepare_static(SQL.into()).await?;
            let rows = conn
                .query(
                    &stmt,
                    &[
                        &ctx.cfg.1,
                        &ToSqlIter(res.iter().map(|x| x.id)),
                        &ToSqlIter(res.iter().map(|x| &*x.author)),
                        &ToSqlIter(res.iter().map(|x| &*x.title)),
                        &ToSqlIter(res.iter().map(|x| x.time)),
                        &ToSqlIter(res.iter().map(|x| x.replies)),
                        &ToSqlIter(res.iter().map(|x| x.views)),
                        &ToSqlIter(res.iter().map(|x| x.lastReply)),
                    ],
                )
                .await?;

            let n_rows = xmax_to_success(rows.iter());

            tracing::info!(target: "db", "\x1b[36m[Page #{page}] update {n_rows}/{} items\x1b[0m", res.len());

            if n_rows == 0 {
                return ControlFlow::Break(());
            }
        };
        if let Err(e) = res {
            tracing::error!(target: "db", "\x1b[31m[Page #{page}] db err: {e}\x1b[0m");
        }
    }

    ControlFlow::Continue(())
}
