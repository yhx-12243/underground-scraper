use core::time::Duration;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        LazyLock,
    },
    time::SystemTime,
};

use compact_str::CompactString;
use fantoccini::{
    error::{CmdError, NewSessionError},
    Client, ClientBuilder,
};
use httpdate::parse_http_date;
use regex::Regex;
use reqwest::{header::DATE, Client as Request};
use serde::Deserialize;
use serde_json::Value;
use tokio::time::sleep;

use crate::db::{get_connection, BB8Error, ToSqlIter};

#[allow(unused)]
pub async fn get_driver(headless: bool) -> Result<Client, NewSessionError> {
    let mut builder = ClientBuilder::native();
    if headless {
        builder.capabilities(
            Some((
                "goog:chromeOptions".to_owned(),
                Value::String("--headless".to_owned()),
            ))
            .into_iter()
            .collect(),
        );
    }
    builder.connect("http://localhost:9515").await
}
/*
#[allow(unused)]
pub async fn fetch(client: &Client, user: &str) -> Result<(SystemTime, XState), CmdError> {
    let url = format!("https://twitter.com/{user}");

    client.goto(&url).await?;

    sleep(Duration::from_secs(2)).await;

    Ok((SystemTime::now(), XState::default()))
}
*/
pub static LIMITED: AtomicBool = AtomicBool::new(false);

static GID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"gt=(\d+);").unwrap());

pub async fn get_guest_token(client: &Request) -> Option<CompactString> {
    for _ in 0..10 {
        let r: Result<_, reqwest::Error> = try {
            client
                .get("https://twitter.com/")
                .send()
                .await?
                .text()
                .await?
        };
        let response = match r {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(target: "get token", "get token failed: {e}, retrying");
                tokio::time::sleep(const { Duration::from_millis(3500) }).await;
                continue;
            }
        };
        let cap = GID.captures(&response);
        let Some(s) = cap.and_then(|c| c.get(1)) else {
            tracing::error!(target: "get token", "parse regex failed, retrying");
            tokio::time::sleep(const { Duration::from_millis(3500) }).await;
            continue;
        };
        let token = CompactString::new(s.as_str());
        tracing::info!(target: "get token", "update token: {token}");
        return Some(token);
    }
    None
}

#[cfg(any())]
pub async fn work(
    manager: &DataManager,
    user: CompactString,
    client: &Request,
    authorization: &str,
    x_guest_token: &str,
) -> Option<CompactString> {
    #[derive(Deserialize)]
    struct XResp {
        data: XResp1,
    }
    #[derive(Deserialize)]
    struct XResp1 {
        user: Option<XResp2>,
    }
    #[derive(Deserialize)]
    struct XResp2 {
        result: Value,
    }
    #[derive(Debug, Deserialize)]
    struct TFormat {
        rest_id: CompactString,
        legacy: TLegacy,
        views: TViews,

    }
    #[derive(Debug, Deserialize)]
    struct TLegacy {
        bookmark_count: i64,
        favorite_count: i64,
        quote_count: i64,
        reply_count: i64,
        retweet_count: i64,
        created_at: String,
    }
    #[derive(Debug, Deserialize)]
    struct TViews {
        count: Option<CompactString>,
    }


    let Some(account) = manager.get(&user) else {
        tracing::error!(target: "worker", "{user} entry is missing");
        return None;
    };

    let url = format!("https://api.twitter.com/graphql/k5XapwcSikNsEsILW5FvgA/UserByScreenName?variables=%7B%22screen_name%22%3A%22{user}%22%2C%22withSafetyModeUserFields%22%3Atrue%7D&features=%7B%22hidden_profile_likes_enabled%22%3Atrue%2C%22hidden_profile_subscriptions_enabled%22%3Atrue%2C%22responsive_web_graphql_exclude_directive_enabled%22%3Atrue%2C%22verified_phone_label_enabled%22%3Afalse%2C%22subscriptions_verification_info_is_identity_verified_enabled%22%3Atrue%2C%22subscriptions_verification_info_verified_since_enabled%22%3Atrue%2C%22highlights_tweets_tab_ui_enabled%22%3Atrue%2C%22responsive_web_twitter_article_notes_tab_enabled%22%3Atrue%2C%22creator_subscriptions_tweet_preview_api_enabled%22%3Atrue%2C%22responsive_web_graphql_skip_user_profile_image_extensions_enabled%22%3Afalse%2C%22responsive_web_graphql_timeline_navigation_enabled%22%3Atrue%7D&fieldToggles=%7B%22withAuxiliaryUserLabels%22%3Afalse%7D");

    let request = client
        .get(url)
        .header("authorization", authorization)
        .header("x-guest-token", x_guest_token);

    let resp = match request.send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(target: "worker", "request for {user} failed: {e:?}");
            return Some(user);
        }
    };

    let Some(date) = resp
        .headers()
        .get(DATE)
        .and_then(|s| s.to_str().ok())
        .and_then(|s| parse_http_date(s).ok())
    else {
        tracing::warn!(target: "worker", "no/wrong date for {user}");
        return Some(user);
    };

    let body = match resp.text().await {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(target: "worker", "body for {user} failed: {e:?}");
            return Some(user);
        }
    };

    let Ok(json) = serde_json::from_str::<XResp>(&body) else {
        if body.trim() == "Rate limit exceeded" {
            tracing::warn!(target: "worker", "expired at {user}, refreshing");
            LIMITED.store(true, Ordering::Release);
        } else {
            tracing::warn!(target: "worker", "{user} result: {}", body.trim());
        }
        return Some(user);
    };

    let Some(u) = json.data.user else {
        tracing::debug!(target: "worker", "{user} not exist");
        account.borrow_mut().insert(date, XState::not_exist());
        return None;
    };

    let Some(l) = u.result.get("legacy") else {
        tracing::debug!(target: "worker", "{user} is suspended");
        account.borrow_mut().insert(date, XState::suspended());
        return None;
    };

    let xstate: Option<XState> = try {
        XState {
            followers: l.get("followers_count")?.as_i64()?,
            following: l.get("friends_count")?.as_i64()?,
            blue: u.result.get("is_blue_verified")?.as_bool()?,
        }
    };
    let Some(xstate) = xstate else {
        tracing::debug!(target: "worker", "{user} data format error: {:?}", u.result);
        return Some(user);
    };

    tracing::debug!(target: "worker", "{user} result: {xstate:?}");
    account.borrow_mut().insert(date, xstate);

    if let Some(uid) = u.result.get("rest_id").and_then(|x| x.as_str()) {
        let url = format!("https://api.twitter.com/graphql/eS7LO5Jy3xgmd3dbL044EA/UserTweets?variables=%7B%22userId%22%3A%22{uid}%22%2C%22count%22%3A20%2C%22includePromotedContent%22%3Atrue%2C%22withQuickPromoteEligibilityTweetFields%22%3Atrue%2C%22withVoice%22%3Atrue%2C%22withV2Timeline%22%3Atrue%7D&features=%7B%22responsive_web_graphql_exclude_directive_enabled%22%3Atrue%2C%22verified_phone_label_enabled%22%3Afalse%2C%22creator_subscriptions_tweet_preview_api_enabled%22%3Atrue%2C%22responsive_web_graphql_timeline_navigation_enabled%22%3Atrue%2C%22responsive_web_graphql_skip_user_profile_image_extensions_enabled%22%3Afalse%2C%22c9s_tweet_anatomy_moderator_badge_enabled%22%3Atrue%2C%22tweetypie_unmention_optimization_enabled%22%3Atrue%2C%22responsive_web_edit_tweet_api_enabled%22%3Atrue%2C%22graphql_is_translatable_rweb_tweet_is_translatable_enabled%22%3Atrue%2C%22view_counts_everywhere_api_enabled%22%3Atrue%2C%22longform_notetweets_consumption_enabled%22%3Atrue%2C%22responsive_web_twitter_article_tweet_consumption_enabled%22%3Atrue%2C%22tweet_awards_web_tipping_enabled%22%3Afalse%2C%22freedom_of_speech_not_reach_fetch_enabled%22%3Atrue%2C%22standardized_nudges_misinfo%22%3Atrue%2C%22tweet_with_visibility_results_prefer_gql_limited_actions_policy_enabled%22%3Atrue%2C%22rweb_video_timestamps_enabled%22%3Atrue%2C%22longform_notetweets_rich_text_read_enabled%22%3Atrue%2C%22longform_notetweets_inline_media_enabled%22%3Atrue%2C%22responsive_web_enhance_cards_enabled%22%3Afalse%7D");

        let request = client
            .get(url)
            .header("authorization", authorization)
            .header("x-guest-token", x_guest_token);

        let resp = match request.send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(target: "worker", "tweets request for {user} failed: {e:?}");
                return None;
            }
        };

        let Some(date) = resp
            .headers()
            .get(DATE)
            .and_then(|s| s.to_str().ok())
            .and_then(|s| parse_http_date(s).ok())
        else {
            tracing::warn!(target: "worker", "no/wrong date for {user}");
            return None;
        };

        let body = match resp.text().await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(target: "worker", "tweets body for {user} failed: {e:?}");
                return None;
            }
        };

        let Ok(XResp {
            data: XResp1 {
                user: Some(XResp2 { mut result }),
            },
        }) = serde_json::from_str::<XResp>(&body)
        else {
            if body.trim() == "Rate limit exceeded" {
                tracing::warn!(target: "worker", "expired at {user} tweets, refreshing");
                LIMITED.store(true, Ordering::Release);
            } else {
                tracing::warn!(target: "worker", "{user} tweets result: {}", body.trim());
            }
            return None;
        };

        let a: Option<&mut Value> = try {
            result
                .get_mut("timeline_v2")?
                .get_mut("timeline")?
                .get_mut("instructions")?
        };
        let Some(Value::Array(instructions)) = a else {
            tracing::warn!(target: "worker", "{user} tweets instructions not found: {result}");
            return None;
        };
        let tweets = instructions
            .iter_mut()
            .filter_map(|instruction| {
                let z = match instruction.get("type")?.as_str()? {
                    "TimelineAddEntries" => instruction.get_mut("entries")?.as_array_mut().map(core::ops::DerefMut::deref_mut),
                    "TimelinePinEntry" => instruction.get_mut("entry").map(core::slice::from_mut),
                    _ => None,
                };
                z
            })
            .flatten();

        let mut archived = Vec::new();
        for (idx, tweet) in tweets.enumerate() {
            let tweet: Option<&mut Value> = try {
                tweet.get_mut("content")?.get_mut("itemContent")?.get_mut("tweet_results")?.get_mut("result")?
            };
            let Some(tweet) = tweet else {
                tracing::warn!(target: "worker", "format error at the {idx}th tweet of {user}");
                continue;
            };
            let mut tweet = match serde_json::from_value::<TFormat>(tweet.take()) {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!(target: "worker", "format error at the {idx}th tweet of {user}: {e}");
                    continue;
                }
            };
        
            archived.push((
                tweet.rest_id.parse().unwrap_or(-1i64),
                unsafe {
                    let s = tweet.legacy.created_at.as_mut_vec();
                    if s.len() != 30 {
                        tracing::warn!(target: "worker", "({user}, {idx}) date format {} error", tweet.legacy.created_at);
                        continue;
                    }
                    s.copy_within(26.., 20);
                    s.truncate(24);
                    match parse_http_date(&tweet.legacy.created_at) {
                        Ok(z) => z,
                        Err(e) => {
                            tracing::warn!(target: "worker", "({user}, {idx}) date format {} error: {e}", tweet.legacy.created_at);
                            continue;
                        }
                    }
                },
                tweet.legacy.bookmark_count,
                tweet.legacy.favorite_count,
                tweet.legacy.quote_count,
                tweet.legacy.reply_count,
                tweet.legacy.retweet_count,
                tweet.views.count.map_or(-1i64, |x| x.parse().unwrap_or(-164)),
            ));
        }

        if !archived.is_empty() {
            let res: Result<(), BB8Error> = try {
                const SQL_TWEET: &str = "with tmp_insert(t, c, b, f, q, r, s, v) as (select * from unnest($1::bigint[], $4::timestamp[], $5::bigint[], $6::bigint[], $7::bigint[], $8::bigint[], $9::bigint[], $10::bigint[])) insert into twitter.tweets (tid, time, \"user\", created_at, bookmark_count, favorite_count, quote_count, reply_count, retweet_count, view_count) select t, $2, $3, c, b, f, q, r, s, v from tmp_insert";

                let mut conn = get_connection().await?;
                let stmt = conn.prepare_static(SQL_TWEET.into()).await?;
                conn.execute(&stmt, &[
                    &ToSqlIter(archived.iter().map(|x| x.0)),
                    &date,
                    &&*user,
                    &ToSqlIter(archived.iter().map(|x| x.1)),
                    &ToSqlIter(archived.iter().map(|x| x.2)),
                    &ToSqlIter(archived.iter().map(|x| x.3)),
                    &ToSqlIter(archived.iter().map(|x| x.4)),
                    &ToSqlIter(archived.iter().map(|x| x.5)),
                    &ToSqlIter(archived.iter().map(|x| x.6)),
                    &ToSqlIter(archived.iter().map(|x| x.7)),
                ]).await?;

                tracing::info!(target: "worker", "\x1b[36m({user}, {uid}): update {} tweets\x1b[0m", archived.len());
            };
            if let Err(e) = res {
                tracing::error!(target: "worker", "\x1b[31m{e}\x1b[0m at user {user}");
            }
        }
    }

    None
}
