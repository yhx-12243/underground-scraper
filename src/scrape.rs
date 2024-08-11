use std::{ffi::OsStr, mem::ManuallyDrop, sync::Arc, time::Duration};

use headless_chrome::{
    browser::tab::NoElementFound, protocol::cdp::Runtime, Browser, Element, LaunchOptions, Tab,
};
use serde_json::Value;
use tokio::{
    task::{block_in_place, spawn_blocking},
    time::sleep,
};

pub const USER_AGENTS: [&str; 19] = [
	"Mozilla/5.0 (Linux; Android 8.1.0; Moto G (4)) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Mobile Safari/537.36 PTST/240201.144844",
	"Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36 Config/92.2.2788.20",
	"Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36 Agency/98.8.8175.80",
	"Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
	"Mozilla/5.0 (Linux; Android 11; moto e20 Build/RONS31.267-94-14) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Mobile Safari/537.36",
	"Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Mobile Safari/537.36",
	"Mozilla/5.0 (iPhone; CPU iPhone OS 17_3 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/124.0.6261.62 Mobile/15E148 Safari/604.1",
	"Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36 Trailer/93.3.3516.28",
	"Mozilla/5.0 (Linux; Android 8.1.0; C5 2019 Build/OPM2.171019.012) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Mobile Safari/537.36",
	"Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
	"Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",

	"Mozilla/5.0 (Windows NT 10.0; WOW64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
	"Mozilla/5.0 (Linux; Android 6.0; Nexus 5 Build/MRA58N) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Mobile Safari/537.36",
	"Mozilla/5.0 (Linux; Android 10; Pixel 4) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Mobile Safari/537.36",
	"Mozilla/5.0 (Linux; Android 4.3; Nexus 7 Build/JSS15Q) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
	"Mozilla/5.0 (iPhone; CPU iPhone OS 13_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/124.0.0.0 Mobile/15E148 Safari/604.1",
	"Mozilla/5.0 (iPad; CPU OS 13_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/124.0.0.0 Mobile/15E148 Safari/604.1",
	"Mozilla/5.0 (X11; CrOS x86_64 10066.0.0) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
	"Mozilla/5.0 (Macintosh; Intel Mac OS X 10_14_6) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
];

pub fn simple() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(const { core::time::Duration::from_secs(5) })
        .build()
}

pub fn puppeteer(headless: bool, proxy: Option<String>) -> anyhow::Result<Browser> {
    Browser::new(LaunchOptions {
        args: vec![OsStr::new("--disable-blink-features=AutomationControlled")],
        headless,
        proxy_server: proxy.as_deref(),
        ..LaunchOptions::default()
    })
}

#[allow(clippy::significant_drop_tightening)]
pub fn first_tab(browser: &Browser) -> anyhow::Result<Arc<Tab>> {
    let tabs_guard = browser
        .get_tabs()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (first, remains) = tabs_guard
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("no tabs found"))?;

    for remain in remains {
        remain.close(true)?;
    }

    Ok(first.clone())
}

pub async fn wait_for_async<'tab>(tab: &'tab Tab, selector: &str) -> anyhow::Result<Element<'tab>> {
    const PERIOD: Duration = Duration::from_millis(1832 / 4);

    loop {
        match block_in_place(|| tab.find_element(selector)) {
            Ok(element) => break Ok(element),
            Err(err) => {
                if !err.is::<NoElementFound>() {
                    break Err(err);
                }
            }
        }

        sleep(PERIOD).await;
    }
}

pub async fn inner_html(element: &Element<'_>) -> anyhow::Result<String> {
    let tab = {
        let tab = ManuallyDrop::new(unsafe { Arc::from_raw(element.parent) });
        Arc::clone(&tab)
    };
    let remote_object_id = element.remote_object_id.clone();

    let ret = spawn_blocking(move ||
        tab.call_method(Runtime::CallFunctionOn {
            object_id: Some(remote_object_id),
            function_declaration: "function(){return this.innerHTML}".to_owned(),
            arguments: Some(Vec::new()),
            return_by_value: Some(false),
            generate_preview: Some(true),
            silent: Some(false),
            await_promise: Some(false),
            user_gesture: None,
            execution_context_id: None,
            object_group: None,
            throw_on_side_effect: None,
        })
    ).await??;

    match ret.result.value {
        Some(Value::String(s)) => Ok(s),
        Some(value) => anyhow::bail!("not a string: {value}"),
        None => anyhow::bail!("returned nothing"),
    }
}
