#![feature(never_type, try_blocks)]

mod service;

#[allow(dead_code)]
async fn get_ids() -> impl Iterator<Item = i64> {
    const SQL: &str = "select distinct id from hackforums.posts natural left outer join hackforums.content where hackforums.content.id is null order by id desc";
    let mut conn = uscr::db::get_connection().await.unwrap();
    let stmt = conn.prepare_static(SQL.into()).await.unwrap();
    conn.query(&stmt, &[])
        .await
        .unwrap()
        .into_iter()
        .filter_map(|row| row.try_get(0).ok())
}

async fn get_black_ids() -> impl Iterator<Item = i64> {
    const SQL: &str = "select distinct id from blackhatworld.posts natural left outer join blackhatworld.content where blackhatworld.content.id is null order by id desc";
    let mut conn = uscr::db::get_connection().await.unwrap();
    let stmt = conn.prepare_static(SQL.into()).await.unwrap();
    conn.query(&stmt, &[])
        .await
        .unwrap()
        .into_iter()
        .filter_map(|row| row.try_get(0).ok())
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    use axum::{
        Router,
        extract::DefaultBodyLimit,
        routing::{get, post},
        serve,
    };
    use tokio::net::UnixListener;
    use tower_http::cors::CorsLayer;

    const SOCK: &str = "underground-scraper.sock";

    pretty_env_logger::init_timed();
    uscr::db::init_db().await;

    service::init(Vec::new(), get_black_ids().await.collect());

    let app: Router = Router::new()
        .route("/get", get(service::get))
        .route("/get/black", get(service::get_black))
        .route("/send", post(service::send))
        .route("/send/black", post(service::send_black))
        .layer(DefaultBodyLimit::disable())
        .layer(CorsLayer::very_permissive().allow_private_network(true));

    if let Err(err) = std::fs::remove_file(SOCK) && err.kind() != std::io::ErrorKind::NotFound {
        return Err(err);
    }

    serve(UnixListener::bind(SOCK)?, app).await
}

/*
const dp = new DOMParser();
const sleep = ms => new Promise(f => setTimeout(f, ms));

async function work(id) {
    const txt = await fetch(`https://hackforums.net/showthread.php?tid=${id}`).then(x => x.text());
    const doc = dp.parseFromString(txt, 'text/html');
    const div = doc.querySelector('.post_content');
    const [head, body] = div.children;
    const date_str = head.querySelector('.post_date').firstChild.textContent;
    const date = new Date(`${date_str}`).getTime();
    const content = body.innerText;
    return { id, date, content };
}

const sleep = ms => new Promise(f => setTimeout(f, ms));

async function work(id) {
    const txt = await fetch(`https://www.blackhatworld.com/seo/${id}`).then(x => x.text());
    // const doc = dp.parseFromString(txt, 'text/html');
    // const div = doc.querySelector('article.message-body');
    // const content = div.innerText;
    return { id, content: txt };
}

async function go(list) {
    const futs = [];
    for (let i = 0; i < list.length; ++i) {
        const idx = i, id = list[i];
        await work(id)
            .then(data => {
                console.log(idx, data.id, 'finished');
                return fetch('https://localhost:18322/send/black', {
                    method: 'POST',
                    body: JSON.stringify(data),
                });
            });
        await sleep(5000);
    }
}

for (let i = 0; ; ++i) {
    list = await fetch('https://localhost:18322/get/black').then(x => x.json());
    if (!list.length) break;
    await go(list).then(() => {}, () => {});
}



*/
