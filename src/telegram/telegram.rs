use std::io::{stdin, stdout, Write};

use grammers_client::{Client, Config, InitParams};
use grammers_session::Session;
use grammers_tl_types as tl;
use tl::types::{messages::ChatFull, InputChannel};

#[allow(clippy::from_str_radix_10)] // false positive (const fn)
const API_ID: i32 = match i32::from_str_radix(env!("TG_ID"), 10) {
    Ok(id) => id,
    Err(_) => panic!("invalid API_ID format"),
};
const API_HASH: &str = env!("TG_HASH");

const SESSION_PATH: &str = "telegram.session";

pub async fn get_client() -> anyhow::Result<Client> {
    let config = Config {
        session: Session::load_file_or_create(SESSION_PATH)?,
        api_id: API_ID,
        api_hash: API_HASH.to_owned(),
        params: InitParams::default(),
    };

    Client::connect(config).await.map_err(Into::into)
}

pub async fn login(client: &Client) -> anyhow::Result<()> {
    let mut phone = String::new();
    while !client.is_authorized().await? {
        if phone.is_empty() {
            let mut stdout = stdout();
            stdout.write_all(b"Please enter your phone: ")?;
            stdout.flush()?;
            stdin().read_line(&mut phone)?;
        }
        let token = client.request_login_code(phone.trim()).await?;

        let mut code = String::new();
        {
            let mut stdout = stdout();
            stdout.write_all(b"Please enter the code you received: ")?;
            stdout.flush()?;
            stdin().read_line(&mut code)?;
        }
        client.sign_in(&token, &code).await?;
    }
    Ok(())
}

pub fn save(client: &Client) -> std::io::Result<()> {
    client.session().save_to_file(SESSION_PATH)
}

pub async fn get_channel_access_hash(client: &Client, channel_id: i64) -> anyhow::Result<i64> {
    use tl::{
        enums::{messages::Chats, Chat, InputChannel::Channel},
        types::messages,
    };

    let id = InputChannel {
        channel_id,
        access_hash: 0,
    };

    let request = tl::functions::channels::GetChannels {
        id: vec![Channel(id)],
    };

    let (Chats::Chats(messages::Chats { chats })
    | Chats::Slice(messages::ChatsSlice { chats, .. })) = client.invoke(&request).await?;

    let Some(Chat::Channel(channel)) = chats.first() else {
        anyhow::bail!("channel #{channel_id} not found");
    };

    Ok(channel.access_hash.unwrap_or(0))
}

pub async fn get_channel_info(client: &Client, channel_id: i64) -> anyhow::Result<ChatFull> {
    use tl::enums::{messages::ChatFull::Full, InputChannel::Channel};

    let access_hash = get_channel_access_hash(client, channel_id).await?;
    let id = InputChannel {
        channel_id,
        access_hash,
    };

    let request = tl::functions::channels::GetFullChannel {
        channel: Channel(id),
    };

    let Full(result) = client.invoke(&request).await?;
    Ok(result)
}
