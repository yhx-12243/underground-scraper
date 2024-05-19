use std::time::{Duration, SystemTime};

use actix_web::{post, web::Json};
use parking_lot::Mutex;
use serde::Deserialize;
use uscr::db::BB8Error;

static IDS: Mutex<Vec<i64>> = Mutex::new(Vec::new());
static BIDS: Mutex<Vec<i64>> = Mutex::new(Vec::new());

pub fn init(ids: Vec<i64>, bids: Vec<i64>) {
    *IDS.lock() = ids;
    *BIDS.lock() = bids;
}

pub fn remove(which: &Mutex<Vec<i64>>, id: i64) {
    let mut guard = which.lock();
    if let Some(i) = guard.iter().position(|&x| x == id) {
        guard.remove(i);
    }
}

#[actix_web::get("/get")]
pub async fn get() -> Json<Vec<i64>> {
    let mut guard = IDS.lock();
    let ret = if guard.len() > 50 {
        guard.rotate_left(50);
        guard[guard.len() - 50..].to_vec()
    } else {
        guard.clone()
    };
    Json(ret)
}

#[actix_web::get("/get/black")]
pub async fn get_black() -> Json<Vec<i64>> {
    let mut guard = BIDS.lock();
    let L = 50.min((guard.len() + 1) / 2);
    // SAFETY: (x + 1) / 2 <= x.
    unsafe {
        core::hint::assert_unchecked(L <= guard.len());
    }
    guard.rotate_left(L);
    Json(guard[guard.len() - L..].to_vec())
}

#[derive(Deserialize)]
struct SendData {
    id: i64,
    date: u64,
    content: String,
}

#[post("/send")]
pub async fn send(data: Json<SendData>) -> Json<String> {
    const SQL: &str =
        "insert into hackforums.content (id, create_time, content) values ($1, $2, $3)";

    let Json(SendData { id, date, content }) = data;

    let date = SystemTime::UNIX_EPOCH + Duration::from_millis(date);

    let e: Result<(), BB8Error> = try {
        let mut conn = uscr::db::get_connection().await?;
        let stmt = conn.prepare_static(SQL.into()).await?;
        conn.execute(&stmt, &[&id, &date, &&*content]).await?;
    };
    if let Err(e) = e {
        return Json(e.to_string());
    }

    remove(&IDS, id);

    Json(String::new())
}

#[derive(Deserialize)]
struct SendDataBlack {
    id: i64,
    content: String,
}

#[post("/send/black")]
pub async fn send_black(data: Json<SendDataBlack>) -> Json<String> {
    const SQL: &str = "insert into blackhatworld.content (id, content) values ($1, $2)";

    let Json(SendDataBlack { id, content }) = data;

    let e: Result<(), BB8Error> = try {
        let mut conn = uscr::db::get_connection().await?;
        let stmt = conn.prepare_static(SQL.into()).await?;
        conn.execute(&stmt, &[&id, &&*content]).await?;
    };
    if let Err(e) = e {
        return Json(e.to_string());
    }

    remove(&BIDS, id);

    Json(String::new())
}
