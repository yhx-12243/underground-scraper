use std::time::{Duration, SystemTime};

use fantoccini::{Client as Driver, Locator};
use scraper::{ElementRef, Html, Selector};
use uscr::{
    db::{get_connection, BB8Error, ToSqlIter},
    util::simple_parse,
};

pub struct Context {
    pub driver: Driver,
    pub sel_content_tr: Selector,
    pub sel_subject_old: Selector,
}

#[derive(Debug)]
pub struct Thread {
    pub tid: i64,
    pub title: String,
    pub replies: i64,
    pub views: i64,
    pub lastPost: SystemTime,
}

pub async fn work(page: i32, ctx: &Context) {
    tracing::info!(target: "worker", "[Page #{page}] start");

    let url = format!(
        // "https://hackforums.net/forumdisplay.php?fid=263&page={page}"
        "https://hackforums.net/forumdisplay.php?fid=291&page={page}"
    );

    if let Err(e) = ctx.driver.goto(&url).await {
        tracing::warn!(target: "worker", "[Page #{page}] err: {e:?}");
        return;
    }

    let locator = Locator::Css("#content table.clear");
    if let Err(e) = ctx.driver.wait().forever().for_element(locator).await {
        tracing::warn!(target: "worker", "[Page #{page}] err: {e:?}");
        return;
    }

    let trs = match ctx.driver.find(locator).await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(target: "worker", "[Page #{page}] err: {e:?}");
            return;
        }
    };

    let html = match trs.html(false).await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(target: "worker", "[Page #{page}] err: {e:?}");
            return;
        }
    };

    let fragment = Html::parse_fragment(&html);
    let res = fragment
        .select(&ctx.sel_content_tr)
        .filter_map(|tr| {
            let c = tr.child_elements().next_chunk::<5>().ok()?;

            let sub = c[1].select(&ctx.sel_subject_old).next()?;
            let title = sub.text().map(str::trim).collect::<String>();
            let tid = sub.attr("id")?.strip_prefix("tid_")?.parse().ok()?;
            let replies = c[2]
                .text()
                .map(str::trim)
                .collect::<String>()
                .replace(',', "")
                .parse()
                .ok()?;
            let views = c[3]
                .text()
                .map(str::trim)
                .collect::<String>()
                .replace(',', "")
                .parse()
                .ok()?;
            let lastPost = {
                let a = c[4].child_elements().next()?.first_child()?;
                if let Some(b) = ElementRef::wrap(a) {
                    SystemTime::UNIX_EPOCH
                        .checked_add(Duration::from_secs(b.attr("data-timestamp")?.parse().ok()?))?
                } else {
                    simple_parse(a.value().as_text()?.as_ascii()?)?
                }
            };

            Some(Thread {
                tid,
                title,
                replies,
                views,
                lastPost,
            })
        })
        .collect::<Vec<_>>();

    if !res.is_empty() {
        let res: Result<(), BB8Error> = try {
            const SQL: &str = "with tmp_insert(i, t, r, v, l) as (select * from unnest($1::bigint[], $2::text[], $3::bigint[], $4::bigint[], $5::timestamp[])) insert into hackforums.posts (id, title, replies, views, lastpost, time, section) select i, t, r, v, l, now() at time zone 'UTC', 291 from tmp_insert";

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

            tracing::info!(target: "db", "\x1b[36m[Page #{page}] update {} items\x1b[0m", res.len());
        };
        if let Err(e) = res {
            tracing::error!(target: "db", "\x1b[31m[Page #{page}] db err: {e}\x1b[0m");
        }
    }
}
