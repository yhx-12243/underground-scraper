use std::{borrow::Cow, ffi::OsStr, sync::Arc, time::Duration};

use headless_chrome::{
    Browser, Element, LaunchOptions, Tab,
    browser::tab::NoElementFound,
    protocol::cdp::{DOM, Runtime},
};
use serde_json::Value;
use tokio::{task::spawn_blocking, time::sleep};

use crate::util::clone_arc;

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
    let tab = browser.new_tab()?;

    {
        let tabs_guard = browser
            .get_tabs()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        for remain in &*tabs_guard {
            if !Arc::ptr_eq(&tab, remain) {
                remain.close(true)?;
            }
        }
    }

    Ok(tab)
}

pub async fn navigate_to(tab: &Tab, url: Cow<'static, str>) -> anyhow::Result<()> {
    let tab = clone_arc(tab);

    spawn_blocking(move || tab.navigate_to(&url).map(|_| ())).await?
}

pub async fn find_async<'tab>(
    tab: &'tab Tab,
    selector: Cow<'static, str>,
) -> anyhow::Result<Element<'tab>> {
    let arc_tab = clone_arc(tab);

    let result = spawn_blocking(move || {
        match arc_tab.find_element(&selector) {
            Ok(element) => Ok((element.remote_object_id, element.backend_node_id, element.node_id, element.attributes, element.tag_name, element.value)),
            Err(err) => Err(err)
        }
    }).await?;

    match result {
        Ok((remote_object_id, backend_node_id, node_id, attributes, tag_name, value)) => Ok(Element { remote_object_id, backend_node_id, node_id, parent: tab, attributes, tag_name, value }),
        Err(err) => Err(err),
    }
}

pub async fn wait_for_async<'tab>(
    tab: &'tab Tab,
    selector: Cow<'static, str>,
) -> anyhow::Result<Element<'tab>> {
    const PERIOD: Duration = Duration::from_millis(1832 / 4);

    loop {
        match find_async(tab, selector.clone()).await {
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
    let tab = clone_arc(element.parent);
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
            serialization_options: None,
            unique_context_id: None,
        })
    ).await??;

    match ret.result.value {
        Some(Value::String(s)) => Ok(s),
        Some(value) => anyhow::bail!("not a string: {value}"),
        None => anyhow::bail!("returned nothing"),
    }
}

pub async fn outer_html(element: &Element<'_>) -> anyhow::Result<String> {
    let tab = clone_arc(element.parent);
    let node_id = element.node_id;
    let backend_node_id = element.backend_node_id;
    let remote_object_id = element.remote_object_id.clone();

    spawn_blocking(move ||
        tab.call_method(DOM::GetOuterHTML {
            node_id: Some(node_id),
            backend_node_id: Some(backend_node_id),
            object_id: Some(remote_object_id),
        })
    ).await?.map(|x| x.outer_html)
}
