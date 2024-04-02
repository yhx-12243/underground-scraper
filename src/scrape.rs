use fantoccini::{error::NewSessionError, Client, ClientBuilder};

#[allow(unused)]
pub async fn get_driver(headless: bool) -> Result<Client, NewSessionError> {
    let mut builder = ClientBuilder::native();
    if headless {
        builder.capabilities(
            #[allow(clippy::iter_on_single_items)]
            Some((
                "goog:chromeOptions".to_owned(),
                serde_json::Value::String("--headless".to_owned()),
            ))
            .into_iter()
            .collect(),
        );
    }
    builder.connect("http://localhost:9515").await
}
