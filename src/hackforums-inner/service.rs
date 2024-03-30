use std::time::{Duration, SystemTime};

use actix_web::{post, web::Json};
use parking_lot::Mutex;
use serde::Deserialize;
use t2::db::BB8Error;

static IDS: Mutex<Vec<i64>> = Mutex::new(Vec::new());

pub fn init(value: Vec<i64>) {
    *IDS.lock() = value;
}

pub fn remove(id: i64) {
    let mut guard = IDS.lock();
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

#[derive(Deserialize)]
struct SendData {
    id: i64,
    date: u64,
    content: String,
}

#[post("/send")]
pub async fn send(data: Json<SendData>) -> Json<String> {
    let Json(SendData { id, date, content }) = data;

    let date = SystemTime::UNIX_EPOCH + Duration::from_millis(date);

    const SQL: &str =
        "insert into hackforums.content (id, create_time, content) values ($1, $2, $3)";
    let e: Result<(), BB8Error> = try {
        let mut conn = t2::db::get_connection().await?;
        let stmt = conn.prepare_static(SQL.into()).await?;
        conn.execute(&stmt, &[&id, &date, &&*content]).await?;
    };
    if let Err(e) = e {
        return Json(e.to_string());
    }

    remove(id);

    Json(String::new())
}
