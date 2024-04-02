#![feature(
    allocator_api,
    inline_const,
    iter_array_chunks,
    iter_next_chunk,
    lazy_cell,
    stmt_expr_attributes,
    try_blocks,
    yeet_expr
)]

mod service;

async fn get_ids() -> impl Iterator<Item = i64> {
    const SQL: &str = "select distinct id from hackforums.posts natural left outer join hackforums.content where hackforums.content.id is null order by id desc";
    let mut conn = t2::db::get_connection().await.unwrap();
    let stmt = conn.prepare_static(SQL.into()).await.unwrap();
    conn.query(&stmt, &[])
        .await
        .unwrap()
        .into_iter()
        .filter_map(|row| row.try_get(0).ok())
}

async fn get_black_ids() -> impl Iterator<Item = i64> {
    const SQL: &str = "select distinct id from blackhatworld.posts natural left outer join blackhatworld.content where blackhatworld.content.id is null order by id desc";
    let mut conn = t2::db::get_connection().await.unwrap();
    let stmt = conn.prepare_static(SQL.into()).await.unwrap();
    conn.query(&stmt, &[])
        .await
        .unwrap()
        .into_iter()
        .filter_map(|row| row.try_get(0).ok())
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    use actix_web::{web, App, HttpServer};

    pretty_env_logger::init_timed();
    t2::db::init_db().await;

    service::init(get_ids().await.collect(), get_black_ids().await.collect());
    // let ids: Vec<i64> = get_ids().await?.collect();

    let json_config = web::JsonConfig::default().content_type_required(false);

    let server = HttpServer::new(move || {
        let cors = actix_cors::Cors::default()
            .allow_any_origin()
            .allow_any_method()
            .allow_any_header()
            .allow_private_network_access();

        App::new()
            .app_data(json_config.clone())
            .wrap(cors)
            .service(service::get)
            .service(service::get_black)
            .service(service::send)
            .service(service::send_black)
    });

    server.bind_uds("underground-scraper.sock")?.run().await
}

/*

// async function work(id) {
//     const txt = await fetch(`https://www.blackhatworld.com/seo/${id}`).then(x => x.text());
//     const doc = dp.parseFromString(txt, 'text/html');
//     const div = doc.querySelector('article.message-body');
//     const content    = div.innerText;
//     return { id, content };
// }

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

async function go(list) {
    const futs = [];
    for (let i = 0; i < list.length; ++i) {
        const idx = i, id = list[i];
        futs.push(
            sleep(idx * 1250)
                .then(() => work(id))
                .then(data => {
                    console.log(idx, data.id, 'finished');
                    return fetch('https://localhost:1832/send', {
                        method: 'POST',
                        body: JSON.stringify(data),
                    });
                })
        );
    }
    return Promise.allSettled(futs);
}

(async () => {


for (let i = 0; ; ++i) {
    list = await fetch('https://localhost:1832/get').then(x => x.json());
    if (!list.length) break;
    await go(list).then(() => {}, () => {});
}


})();

*/
