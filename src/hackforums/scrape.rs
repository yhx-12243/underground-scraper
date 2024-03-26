use std::time::{Duration, SystemTime};

use fantoccini::{Client as Driver, Locator};
use httpdate::parse_http_date;
use scraper::{ElementRef, Html, Selector};

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

fn simple_parse(time_str: &str) -> Option<SystemTime> {
    let mut time = time_str.as_bytes();
    let mut buf = *b"Sun, 06 Nov 1994 08:49:00 GMT";
    buf[8..11].copy_from_slice(&time[..3]);
    if time[5] == b',' {
        buf[6] = time[4];
        time = &time[7..];
    } else {
        buf[5] = time[4];
        buf[6] = time[5];
        time = &time[8..];
    }
    buf[12..22].copy_from_slice(&time[..10]);
    if time[11] == b'P' {
        buf[17] += 1;
        buf[18] += 2;
        if buf[18] >= 58 {
            buf[17] += 1;
            buf[18] -= 10;
        }
    }

    let buf = unsafe { core::str::from_utf8_unchecked(&buf) };
    let date = parse_http_date(buf);
    tracing::info!(target: "time-converter", "{time_str:?} -> {buf:?} -> {date:?}");
    
    date.ok()
}

pub async fn work(page: i32, ctx: &Context) -> Vec<Thread> {
    tracing::info!(target: "worker", "[Page #{page}] start");

    let url = format!("https://hackforums.net/forumdisplay.php?fid=263&page={page}");

    if let Err(e) = ctx.driver.goto(&url).await {
        tracing::warn!(target: "worker", "[Page #{page}] err: {e:?}");
        return Vec::new();
    }

    let locator = Locator::Css("#content table");
    if let Err(e) = ctx.driver.wait().forever().for_element(locator).await {
        tracing::warn!(target: "worker", "[Page #{page}] err: {e:?}");
        return Vec::new();
    }

    let trs = match ctx.driver.find(locator).await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(target: "worker", "[Page #{page}] err: {e:?}");
            return Vec::new();
        }
    };

    let html = match trs.html(false).await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(target: "worker", "[Page #{page}] err: {e:?}");
            return Vec::new();
        }
    };

    let fragment = Html::parse_fragment(&html);
    fragment
        .select(&ctx.sel_content_tr)
        .filter_map(|tr| {
            let c = tr.child_elements().next_chunk::<5>().ok()?;

            let sub = c[1].select(&ctx.sel_subject_old).next()?;
            let mut title = sub.text().collect::<String>();
            unsafe {
                let u = title.trim();
                let (p, l) = (u.as_ptr(), u.len());
                core::ptr::copy(p, title.as_mut_ptr(), l);
                title.as_mut_vec().truncate(l);
            }
            let tid = sub.attr("id")?.strip_prefix("tid_")?.parse().ok()?;
            let replies = c[2]
                .text()
                .collect::<String>()
                .trim()
                .replace(',', "")
                .parse()
                .ok()?;
            let views = c[3]
                .text()
                .collect::<String>()
                .trim()
                .replace(',', "")
                .parse()
                .ok()?;
            let lastPost = {
                let a = c[4].child_elements().next()?.first_child()?;
                if let Some(b) = ElementRef::wrap(a) {
                    SystemTime::UNIX_EPOCH
                        .checked_add(Duration::from_secs(b.attr("data-timestamp")?.parse().ok()?))?
                } else {
                    simple_parse(a.value().as_text()?)?
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
        .collect()
}
