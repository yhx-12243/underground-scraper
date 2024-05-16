use core::{mem::take, time::Duration};

use compact_str::{format_compact, CompactString};
use rand::{thread_rng, Rng};
use reqwest::{
    header::{HeaderValue, COOKIE},
    Client, Proxy, Version,
};
use serde::Serialize;

use crate::{ConfigHeaders, PROXY_HOST, PROXY_PASSWORD, PROXY_USERNAME};

pub struct Worker {
    pub client_port: u16,
    pub server_port: u16,
    pub headers: ConfigHeaders,
    pub gateway: Client,
}

impl Worker {
    fn build_proxy(&self) -> Option<Proxy> {
        let url = format!("http://{}:{}", PROXY_HOST?, self.client_port);
        let proxy = Proxy::all(url).ok()?;
        Some(
            if let Some((username, password)) = PROXY_USERNAME.zip(PROXY_PASSWORD) {
                proxy.basic_auth(username, password)
            } else {
                proxy
            },
        )
    }

    async fn fetch_work(&self) -> reqwest::Result<Vec<i64>> {
        let url = format!("https://localhost:{}/get/black", self.server_port);
        self.gateway.get(url).send().await?.json().await
    }

    async fn submit(&self, id: i64, content: &str) -> reqwest::Result<CompactString> {
        #[derive(Serialize)]
        struct Payload<'a> {
            id: i64,
            content: &'a str,
        }
        let url = format!("https://localhost:{}/send/black", self.server_port);
        self.gateway
            .post(url)
            .json(&Payload { id, content })
            .send()
            .await?
            .json()
            .await
    }

    fn simple_check(text: &str) -> bool {
        text.find("BlackHatWorld</title>").is_some_and(|i|
            // SAFETY: 0 <= i < text.len().
            unsafe { text.get_unchecked(..i).contains("<title>") })
    }

    pub async fn into_future(mut self) -> reqwest::Result<()> {
        let proxy = self.build_proxy();
        let mut client =
            Client::builder().connect_timeout(const { core::time::Duration::from_secs(8) });
        if let Some(proxy) = proxy {
            client = client.proxy(proxy);
        }
        let client = client
            .default_headers(
                if let Ok(cookie) = HeaderValue::try_from(take(&mut self.headers.cookie)) {
                    Some((COOKIE, cookie))
                } else {
                    None
                }
                .into_iter()
                .collect(),
            )
            .user_agent(take(&mut self.headers.user_agent))
            .build()?;
        let target = format_compact!("worker-{}", self.client_port);
        let mut rng = thread_rng();

        loop {
            let works = loop {
                match self.fetch_work().await {
                    Ok(r) => break r,
                    Err(e) => {
                        log::error!(target: &target, "fetch work error: {e:?}");
                        tokio::time::sleep(const { Duration::from_secs(3) }).await;
                    }
                }
            };
            if works.is_empty() {
                return Ok(());
            }
            for work in works {
                let url = format!("https://www.blackhatworld.com/seo/{work}");
                // let url = format!("https://localhost:4433/{work}");
                log::info!(target: &target, "\x1b[33mscraping\x1b[0m {url} ...");

                let response: reqwest::Result<String> = try {
                    client
                        .get(&url)
                        .version(Version::HTTP_2)
                        .send()
                        .await?
                        .text()
                        .await?
                };
                let sleep = match response {
                    Ok(text) if Self::simple_check(&text) => {
                        loop {
                            match self.submit(work, &text).await {
                                Ok(result) if result.is_empty() => {
                                    log::info!(target: &target, "\x1b[36mfinished\x1b[0m {url} ...");
                                    break;
                                }
                                Ok(reason) => {
                                    log::info!(target: &target, "\x1b[31merror\x1b[0m {url}: {reason}");
                                    break;
                                }
                                Err(err) => {
                                    log::error!(target: &target, "\x1b[32msubmit error\x1b[0m {url}: \x1b[35m{text:?}\x1b[0m {err:?}");
                                    tokio::time::sleep(Duration::from_millis(1250)).await;
                                }
                            }
                        }
                        let t = rng.gen_range(2400..3000);
                        Duration::from_millis(t)
                    }
                    Ok(text) => {
                        log::warn!(target: &target, "\x1b[31mwrong\x1b[0m {url}: {text}");
                        const { Duration::from_secs(4) }
                    }
                    Err(e) => {
                        log::error!(target: &target, "fetch error: {e:?}");
                        const { Duration::from_secs(4) }
                    }
                };
                tokio::time::sleep(sleep).await;
            }
        }
    }
}
