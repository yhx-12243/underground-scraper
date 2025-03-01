use grammers_tl_types as tl;
use serde::{Deserialize, Serialize};

type Media = ();
type Entity = ();

#[derive(Debug, Deserialize, Serialize)]
pub struct Button {
    pub text: String,
    pub url: Option<String>,
}

impl From<tl::enums::KeyboardButton> for Button {
    #[rustfmt::skip]
    fn from(button: tl::enums::KeyboardButton) -> Self {
        use tl::enums::KeyboardButton;
        match button {
            | KeyboardButton::Button(tl::types::KeyboardButton { text })
            | KeyboardButton::Callback(tl::types::KeyboardButtonCallback { text, .. })
            | KeyboardButton::RequestPhone(tl::types::KeyboardButtonRequestPhone { text })
            | KeyboardButton::RequestGeoLocation(tl::types::KeyboardButtonRequestGeoLocation { text })
            | KeyboardButton::SwitchInline(tl::types::KeyboardButtonSwitchInline { text, .. })
            | KeyboardButton::Game(tl::types::KeyboardButtonGame { text })
            | KeyboardButton::Buy(tl::types::KeyboardButtonBuy { text })
            | KeyboardButton::RequestPoll(tl::types::KeyboardButtonRequestPoll { text, .. })
            | KeyboardButton::InputKeyboardButtonUserProfile(tl::types::InputKeyboardButtonUserProfile { text, .. })
            | KeyboardButton::UserProfile(tl::types::KeyboardButtonUserProfile { text, .. })
            | KeyboardButton::RequestPeer(tl::types::KeyboardButtonRequestPeer { text, .. })
            | KeyboardButton::InputKeyboardButtonRequestPeer(tl::types::InputKeyboardButtonRequestPeer { text, .. })
            | KeyboardButton::Copy(tl::types::KeyboardButtonCopy { text, .. }) => Self { text, url: None },
            | KeyboardButton::WebView(tl::types::KeyboardButtonWebView { text, url })
            | KeyboardButton::SimpleWebView(tl::types::KeyboardButtonSimpleWebView { text, url })
            | KeyboardButton::Url(tl::types::KeyboardButtonUrl { text, url })
            | KeyboardButton::UrlAuth(tl::types::KeyboardButtonUrlAuth { text, url, .. })
            | KeyboardButton::InputKeyboardButtonUrlAuth(tl::types::InputKeyboardButtonUrlAuth { text, url, .. }) => Self { text, url: Some(url) },
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ReplyMarkup {
    pub single_use: Option<bool>,
    pub selective: Option<bool>,
    pub placeholder: Option<String>,
    pub resize: Option<bool>,
    pub persistent: Option<bool>,
    pub rows: Vec<Vec<Button>>,
}

impl From<tl::enums::ReplyMarkup> for ReplyMarkup {
    #[rustfmt::skip]
    fn from(markup: tl::enums::ReplyMarkup) -> Self {
        use tl::enums::ReplyMarkup;
        match markup {
            ReplyMarkup::ReplyKeyboardHide(tl::types::ReplyKeyboardHide { selective }) => Self { single_use: None, selective: Some(selective), placeholder: None, resize: None, persistent: None, rows: Vec::new() },
            ReplyMarkup::ReplyKeyboardForceReply(tl::types::ReplyKeyboardForceReply { single_use, selective, placeholder }) => Self { single_use: Some(single_use), selective: Some(selective), placeholder, resize: None, persistent: None, rows: Vec::new() },
            ReplyMarkup::ReplyKeyboardMarkup(tl::types::ReplyKeyboardMarkup { resize, single_use, selective, persistent, rows, placeholder }) => Self { single_use: Some(single_use), selective: Some(selective), placeholder, resize: Some(resize), persistent: Some(persistent), rows: rows.into_iter().map(|tl::enums::KeyboardButtonRow::Row(tl::types::KeyboardButtonRow { buttons })| buttons.into_iter().map(Into::into).collect()).collect() },
            ReplyMarkup::ReplyInlineMarkup(tl::types::ReplyInlineMarkup { rows }) => Self { single_use: None, selective: None, placeholder: None, resize: None, persistent: None, rows: rows.into_iter().map(|tl::enums::KeyboardButtonRow::Row(tl::types::KeyboardButtonRow { buttons })| buttons.into_iter().map(Into::into).collect()).collect() },
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[repr(transparent)]
pub struct Peer(pub i64);

impl From<tl::enums::Peer> for Peer {
    fn from(peer: tl::enums::Peer) -> Self {
        use tl::{
            enums::Peer,
            types::{PeerChannel, PeerChat, PeerUser},
        };
        let (Peer::Channel(PeerChannel { channel_id: id })
        | Peer::Chat(PeerChat { chat_id: id })
        | Peer::User(PeerUser { user_id: id })) = peer;
        Self(id)
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct Message {
    pub out: bool,
    pub mentioned: bool,
    pub media_unread: bool,
    pub silent: bool,
    pub post: bool,
    pub from_scheduled: bool,
    pub legacy: bool,
    pub edit_hide: bool,
    pub pinned: bool,
    pub noforwards: bool,
    pub invert_media: bool,
    pub offline: bool,
    pub id: i32,
    pub from_id: Option<Peer>,
    pub from_boosts_applied: Option<i32>,
    pub peer_id: Peer,
    pub saved_peer_id: Option<Peer>,
    pub fwd_from: Option<()>, // Option<crate::enums::MessageFwdHeader>,
    pub via_bot_id: Option<i64>,
    pub via_business_bot_id: Option<i64>,
    pub reply_to: Option<()>, // Option<crate::enums::MessageReplyHeader>,
    pub date: i32,
    #[allow(clippy::struct_field_names)]
    pub message: String,
    pub media: Option<Media>, // ?
    pub reply_markup: Option<ReplyMarkup>,
    pub entities: Option<Vec<Entity>>, // ?
    pub views: Option<i32>,
    pub forwards: Option<i32>,
    pub replies: Option<()>, // Option<crate::enums::MessageReplies>,
    pub edit_date: Option<i32>,
    pub post_author: Option<String>,
    pub grouped_id: Option<i64>,
    pub reactions: Option<()>, // Option<crate::enums::MessageReactions>,
    pub restriction_reason: Option<Vec<()>>, // Option<Vec<crate::enums::RestrictionReason>>,
    pub ttl_period: Option<i32>,
    pub quick_reply_shortcut_id: Option<i32>,
}

impl From<tl::types::Message> for Message {
    fn from(message: tl::types::Message) -> Self {
        Self {
            out: message.out,
            mentioned: message.mentioned,
            media_unread: message.media_unread,
            silent: message.silent,
            post: message.post,
            from_scheduled: message.from_scheduled,
            legacy: message.legacy,
            edit_hide: message.edit_hide,
            pinned: message.pinned,
            noforwards: message.noforwards,
            invert_media: message.invert_media,
            offline: message.offline,
            id: message.id,
            from_id: message.from_id.map(Into::into),
            from_boosts_applied: message.from_boosts_applied,
            peer_id: message.peer_id.into(),
            saved_peer_id: message.saved_peer_id.map(Into::into),
            fwd_from: message.fwd_from.map(|_| ()),
            via_bot_id: message.via_bot_id,
            via_business_bot_id: message.via_business_bot_id,
            reply_to: message.reply_to.map(|_| ()),
            date: message.date,
            message: message.message,
            media: message.media.map(|_| ()),
            reply_markup: message.reply_markup.map(Into::into),
            entities: message
                .entities
                .map(|entities| entities.into_iter().map(|_| ()).collect()),
            views: message.views,
            forwards: message.forwards,
            replies: message.replies.map(|_| ()),
            edit_date: message.edit_date,
            post_author: message.post_author,
            grouped_id: message.grouped_id,
            reactions: message.reactions.map(|_| ()),
            restriction_reason: message
                .restriction_reason
                .map(|reason| reason.into_iter().map(|_| ()).collect()),
            ttl_period: message.ttl_period,
            quick_reply_shortcut_id: message.quick_reply_shortcut_id,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct BotCommand {
    pub command: String,
    pub description: String,
}

impl From<tl::enums::BotCommand> for BotCommand {
    fn from(tl::enums::BotCommand::Command(cmd): tl::enums::BotCommand) -> Self {
        Self {
            command: cmd.command,
            description: cmd.description,
        }
    }
}
