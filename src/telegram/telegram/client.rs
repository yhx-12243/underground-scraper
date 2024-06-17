use std::{
    io::{self, stdin, stdout, Write as _},
    path::{Path, PathBuf},
};

use grammers_client::{Client as ClientInner, Config, InitParams, SignInError};
use grammers_session::Session;
use hashbrown::HashMap;

pub struct Client {
    pub inner: ClientInner,
    session_path: PathBuf,
}

impl Client {
    pub async fn new(
        session_path: &Path,
        api_id: i32,
        api_hash: String,
        flood_sleep_threshold: u32,
    ) -> io::Result<Self> {
        let config = Config {
            session: Session::load_file_or_create(session_path)?,
            api_id,
            api_hash,
            params: InitParams {
                flood_sleep_threshold,
                ..InitParams::default()
            },
        };

        match ClientInner::connect(config).await {
            Ok(inner) => Ok(Self {
                inner,
                session_path: session_path.to_path_buf(),
            }),
            Err(err) => Err(io::Error::other(err)),
        }
    }

    pub async fn login(&self) -> io::Result<()> {
        let mut phone = String::with_capacity(32);
        while !self.inner.is_authorized().await.map_err(io::Error::other)? {
            if phone.is_empty() {
                let mut stdout = stdout();
                stdout.write_all(b"Please enter your phone: ")?;
                stdout.flush()?;
                stdin().read_line(&mut phone)?;
            }
            let token = self
                .inner
                .request_login_code(phone.trim())
                .await
                .map_err(io::Error::other)?;

            let mut code = String::with_capacity(32);
            {
                let mut stdout = stdout();
                stdout.write_all(b"Please enter the code you received: ")?;
                stdout.flush()?;
                stdin().read_line(&mut code)?;
            }
            match self.inner.sign_in(&token, code.trim()).await {
                Ok(_) => (),
                Err(SignInError::PasswordRequired(password_token)) => {
                    let password = {
                        let mut stdout = stdout();
                        if let Some(hint) = password_token.hint() {
                            write!(stdout, "Please enter your password (hint: {hint}): ")
                        } else {
                            stdout.write_all(b"Please enter your password: ")
                        }?;
                        stdout.flush()?;
                        rpassword::read_password_from_bufread(&mut stdin().lock())
                    }?;
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
    map: HashMap<i32, String>,
    mut session_dir: PathBuf,
    flood_sleep_threshold: u32,
) -> HashMap<i32, Client> {
    use uscr::util::SetLenExt;

    let mut clients = HashMap::with_capacity(map.len());
    let dir_len = session_dir.as_os_str().len();
    for (id, hash) in map {
        unsafe {
            session_dir.set_len(dir_len);
        }
        session_dir.append_i32(id);

        let client = match Client::new(&session_dir, id, hash, flood_sleep_threshold).await {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(target: "client-setup(get_client)", id, ?e);
                continue;
            }
        };
        if let Err(e) = client.login().await {
            tracing::error!(target: "client-setup(login)", id, ?e);
            continue;
        }
        if let Err(e) = client.save() {
            tracing::error!(target: "client-setup(save)", id, ?e);
            continue;
        }
        eprintln!(
            "\x1b[33m{id}\x1b[0m => \x1b[36m{:#?}\x1b[0m",
            client.inner.session().get()
        );
        clients.insert_unique_unchecked(id, client);
    }
    clients
}
