use tokio_postgres::types::Json;
use uscr::db::get_connection;

use crate::telegram::Message;

pub async fn extract_content(id: i64, limit: u32) -> Result<(), uscr::db::BB8Error> {
	const SQL: &str = "select data from telegram.message where channel_id = $1 order by message_id desc limit $2";

	let mut conn = get_connection().await?;
	let stmt = conn.prepare_static(SQL.into()).await?;
	let rows = conn.query(&stmt, &[&id, &(limit as i64)]).await?;

	for row in rows {
		let Ok(Json(message)) = row.try_get::<_, Json<Message>>(0) else { continue; };
		let content = message.message;
		println!("{content}");
	}

	Ok(())
}
