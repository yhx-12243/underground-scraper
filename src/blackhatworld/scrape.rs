use std::time::{Duration, SystemTime};

use fantoccini::{Client as Driver, Locator};
use regex::Regex;
use scraper::{Html, Selector};
use t2::db::{get_connection, BB8Error, ToSqlIter};

pub struct Context {
    pub driver: Driver,
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

pub async fn work(page: i32, ctx: &Context) {
    tracing::info!(target: "worker", "[Page #{page}] start");

    let url = format!(
        "https://www.blackhatworld.com/forums/social-media.200/page-{page}"
        // "https://www.blackhatworld.com/forums/content-copywriting.194/page-{page}"
    );

    if let Err(e) = ctx.driver.goto(&url).await {
        tracing::warn!(target: "worker", "[Page #{page}] err: {e:?}");
        return;
    }

    let locator = Locator::Css(".js-threadList");
    if let Err(e) = ctx.driver.wait().forever().for_element(locator).await {
        tracing::warn!(target: "worker", "[Page #{page}] err: {e:?}");
        return;
    }

    let list = match ctx.driver.find(Locator::Css(".structItemContainer")).await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(target: "worker", "[Page #{page}] err: {e:?}");
            return;
        }
    };

    let html = match list.html(false).await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(target: "worker", "[Page #{page}] err: {e:?}");
            return;
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
            const SQL: &str = "with tmp_insert(i, a, t, c, r, v, l) as (select * from unnest($1::bigint[], $2::text[], $3::text[], $4::timestamp[], $5::bigint[], $6::bigint[], $7::timestamp[])) insert into blackhatworld.posts (id, time, author, title, create_time, replies, views, last_reply, section) select i, now() at time zone 'UTC', a, t, c, r, v, l, 200 from tmp_insert";

            let mut conn = get_connection().await?;
            let stmt = conn.prepare_static(SQL.into()).await?;
            conn.execute(
                &stmt,
                &[
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

            tracing::info!(target: "db", "\x1b[36m[Page #{page}] update {} items\x1b[0m", res.len());
        };
        if let Err(e) = res {
            tracing::error!(target: "db", "\x1b[31m[Page #{page}] db err: {e}\x1b[0m");
        }
    }
}
