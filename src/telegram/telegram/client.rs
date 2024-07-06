use std::{
    io::{self, stdin, stdout, BufRead, Write},
    os::fd::{AsFd, AsRawFd},
    path::PathBuf,
};

use grammers_client::{Client as ClientInner, Config, InitParams, SignInError};
use grammers_session::Session;
use hashbrown::HashMap;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct InitConfig {
    pub id: i32,
    pub hash: String,
    pub proxy: Option<String>,
    pub session_file: Option<i32>,
}

#[derive(Debug)]
pub struct Client {
    pub inner: ClientInner,
    session_path: PathBuf,
}

impl Client {
    pub async fn new(
        session_path: PathBuf,
        api_id: i32,
        api_hash: String,
        proxy_url: Option<String>,
        flood_sleep_threshold: u32,
    ) -> io::Result<Self> {
        let config = Config {
            session: Session::load_file_or_create(&session_path)?,
            api_id,
            api_hash,
            params: InitParams {
                flood_sleep_threshold,
                proxy_url,
                ..InitParams::default()
            },
        };

        match ClientInner::connect(config).await {
            Ok(inner) => Ok(Self { inner, session_path }),
            Err(err) => Err(io::Error::other(err)),
        }
    }

    pub async fn login(&self) -> io::Result<()> {
        let mut phone = String::with_capacity(32);
        while !self.inner.is_authorized().await.map_err(io::Error::other)? {
            if phone.is_empty() {
                stdout().write_all(b"Please enter your phone: ")?;
                stdin().read_line(&mut phone)?;
            }
            let token = self
                .inner
                .request_login_code(phone.trim())
                .await
                .map_err(io::Error::other)?;

            let mut code = String::with_capacity(32);
            stdout().write_all(b"Please enter the code you received: ")?;
            stdin().read_line(&mut code)?;
            match self.inner.sign_in(&token, code.trim()).await {
                Ok(_) => (),
                Err(SignInError::PasswordRequired(password_token)) => {
                    if let Some(hint) = password_token.hint() {
                        write!(stdout(), "Please enter your password (hint: {hint}): ")
                    } else {
                        stdout().write_all(b"Please enter your password: ")
                    }?;
                    let mut password = String::with_capacity(32);
                    {
                        let mut stdin = stdin().lock();
                        let fd = stdin.as_fd().as_raw_fd();
                        let _hi = rpassword::HiddenInput::new(fd);
                        stdin.read_line(&mut password)?;
                    };
                    self.inner
                        .check_password(password_token, password.trim())
                        .await
                        .map_err(io::Error::other)?;
                }
                Err(e) => return Err(io::Error::other(e)),
            }
        }
        Ok(())
    }

    pub fn save(&self) -> io::Result<()> {
        self.inner.session().save_to_file(&*self.session_path)
    }
}

pub async fn init_clients_from_map(
    mut configs: Vec<InitConfig>,
    mut session_dir: PathBuf,
    flood_sleep_threshold: u32,
) -> HashMap<i32, Client> {
    use uscr::util::SetLenExt;

    let mut clients = HashMap::with_capacity(configs.len());
    let dir_len = session_dir.as_os_str().len();

    let client_futures = configs.iter_mut().map(|init_config| {
        let api_id = init_config.id;
        unsafe {
            session_dir.set_len(dir_len);
        }
        let session_file = init_config.session_file.unwrap_or(api_id);
        session_dir.append_i32(session_file);

        Client::new(
            session_dir.clone(),
            api_id,
            core::mem::take(&mut init_config.hash),
            init_config.proxy.take(),
            flood_sleep_threshold,
        )
    });

    let client_resolve = futures_util::future::join_all(client_futures).await;

    for (init_config, try_client) in configs.into_iter().zip(client_resolve) {
        let api_id = init_config.id;
        let client = match try_client {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(target: "client-setup(get_client)", api_id, ?e);
                continue;
            }
        };
        if let Err(e) = client.login().await {
            tracing::error!(target: "client-setup(login)", api_id, ?e);
            continue;
        }
        if let Err(e) = client.save() {
            tracing::error!(target: "client-setup(save)", api_id, ?e);
            continue;
        }
        let session_file = init_config.session_file.unwrap_or(api_id);
        match clients.try_insert(session_file, client) {
            Ok(client) => tracing::info!(target: "client-setup(insert)",
                "\x1b[33m{api_id}\x1b[0m (key: \x1b[32m{session_file}\x1b[0m) => \x1b[36m{:?}\x1b[0m",
                client.inner.session().get(),
            ),
            Err(e) => tracing::error!(target: "client-setup(insert)", api_id, ?e),
        }
    }
    clients
}
