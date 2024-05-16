use std::sync::Arc;

use headless_chrome::{
    browser::tab::EventListener,
    protocol::cdp::{
        types::Event,
        Network::{
            self,
            events::{RequestWillBeSentEvent, RequestWillBeSentExtraInfoEvent},
        },
    },
    Tab,
};
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;

use crate::ConfigHeaders;

pub struct NetworkListener {
    tx: UnboundedSender<ConfigHeaders>,
}

impl EventListener<Event> for NetworkListener {
    fn on_event(&self, event: &Event) {
        #[rustfmt::skip]
        let headers = match event {
            Event::NetworkRequestWillBeSent(RequestWillBeSentEvent { params }) => &params.request.headers,
            Event::NetworkRequestWillBeSentExtraInfo(RequestWillBeSentExtraInfoEvent { params }) => &params.headers,
            _ => return,
        };

        let Some(Value::Object(ref headers)) = headers.0 else {
            return;
        };

        let mut cookie = None;
        let mut user_agent = None;
        for (k, v) in headers {
            if let Value::String(s) = v {
                if k.eq_ignore_ascii_case("cookie") {
                    cookie = Some(s);
                } else if k.eq_ignore_ascii_case("user-agent") {
                    user_agent = Some(s);
                }
            }
        }
        if let Some((cookie, user_agent)) = cookie.zip(user_agent) {
            let _ = self.tx.send(ConfigHeaders {
                cookie: cookie.clone(),
                user_agent: user_agent.clone(),
            });
        }
    }
}

pub struct Browser {
    tab: Arc<Tab>,
    tx: UnboundedSender<ConfigHeaders>,
    user_agent: &'static str,
}

impl Browser {
    pub fn new(
        tab: Arc<Tab>,
        tx: UnboundedSender<ConfigHeaders>,
        user_agent: &'static str,
    ) -> Self {
        Self {
            tab,
            tx,
            user_agent,
        }
    }

    pub fn into_work(self) {
        let _ = self.tab.call_method(Network::Enable {
            max_total_buffer_size: None,
            max_resource_buffer_size: None,
            max_post_data_size: None,
        });
        let _ = self.tab.call_method(Network::SetUserAgentOverride {
            user_agent: self.user_agent.to_owned(),
            accept_language: None,
            platform: None,
            user_agent_metadata: None,
        });
        let listener = NetworkListener {
            tx: self.tx.clone(),
        };
        let _ = self.tab.add_event_listener(Arc::new(listener));
    }
}
