use core::fmt::Debug;
use std::sync::OnceLock;

use bb8_postgres::{PostgresConnectionManager, bb8};
use tokio_postgres::{
    NoTls, Row,
    types::{FromSql, IsNull, Kind, ToSql, Type, to_sql_checked},
};

pub type ConnectionManager = PostgresConnectionManager<NoTls>;
pub type Pool = bb8::Pool<ConnectionManager>;
pub type PooledConnection = bb8::PooledConnection<'static, ConnectionManager>;
pub type DBError = tokio_postgres::Error;
pub type BB8Error = bb8::RunError<DBError>;
pub type DBResult<T> = Result<T, DBError>;

static POOL: OnceLock<Pool> = OnceLock::new();

mod constants {
    use core::time::Duration;

    macro_rules! env_or_default {
        ($name:expr, $default:expr) => {
            if let Some(s) = option_env!($name) {
                s
            } else {
                $default
            }
        };
    }

    pub const HOST: &str = env_or_default!("DB_HOST", "/var/run/postgresql");
    pub const USER: &str = env_or_default!("DB_USER", "postgres");
    pub const DBNAME: &str = env_or_default!("DB_NAME", "postgres");
    pub const PASSWORD: Option<&str> = option_env!("DB_PASSWORD");
    pub const CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);
}

pub async fn init_db() {
    use constants::{CONNECTION_TIMEOUT, DBNAME, HOST, PASSWORD, USER};

    let mut config = tokio_postgres::Config::new();
    config
        .host_path(HOST)
        .user(USER)
        .dbname(DBNAME)
        .connect_timeout(CONNECTION_TIMEOUT);
    if let Some(password) = PASSWORD {
        config.password(password);
    }

    let manager = PostgresConnectionManager::new(config, NoTls);

    #[allow(clippy::unwrap_used)]
    let pool = Pool::builder()
        .connection_timeout(CONNECTION_TIMEOUT)
        .build(manager)
        .await
        .unwrap();

    POOL.set(pool).unwrap();
}

#[inline(always)]
pub fn get_connection() -> impl Future<Output = Result<PooledConnection, BB8Error>> {
    unsafe { POOL.get().unwrap_unchecked().get() }
}

#[inline(always)]
pub async fn insert_connection(
    conn: &mut Option<PooledConnection>,
) -> Result<&mut PooledConnection, BB8Error> {
    Ok(if let Some(db) = conn {
        db
    } else {
        conn.insert(get_connection().await?)
    })
}

#[inline]
pub fn transfer_type<'a, T, U>(row: &'a Row, idx: usize) -> DBResult<U>
where
    T: FromSql<'a> + TryInto<U>,
    <T as TryInto<U>>::Error: core::error::Error + Send + Sync + 'static,
{
    row.try_get::<'a, usize, T>(idx)?
        .try_into()
        .map_err(|e| DBError::new(tokio_postgres::error::Kind::FromSql(idx), Some(Box::new(e))))
}

#[repr(transparent)]
pub struct JsonChecked<'a>(pub &'a [u8]);

impl<'a> FromSql<'a> for JsonChecked<'a> {
    #[inline]
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<Self, Box<dyn core::error::Error + Sync + Send>> {
        if let [1, rest @ ..] = raw {
            Ok(Self(rest))
        } else {
            Err("database JSONB error".into())
        }
    }

    #[inline]
    fn accepts(_: &Type) -> bool {
        true
    }
}

#[derive(Debug)]
#[repr(transparent)]
pub struct ToSqlIter<T>(pub T);

impl<T, U> ToSql for ToSqlIter<T>
where
    T: ExactSizeIterator<Item = U> + Clone + Debug,
    U: ToSql,
{
    fn to_sql(
        &self,
        ty: &Type,
        out: &mut bytes::BytesMut,
    ) -> Result<IsNull, Box<dyn std::error::Error + Sync + Send>> {
        let Kind::Array(member_type) = ty.kind() else {
            panic!("expected array type")
        };

        let lower_bound = match *ty {
            Type::OID_VECTOR | Type::INT2_VECTOR => 0,
            _ => 1,
        };

        let dimension = postgres_protocol::types::ArrayDimension {
            len: self.0.len().try_into()?,
            lower_bound,
        };

        postgres_protocol::types::array_to_sql(
            Some(dimension),
            member_type.oid(),
            self.0.clone(),
            |e, w| match e.to_sql(member_type, w)? {
                IsNull::No => Ok(postgres_protocol::IsNull::No),
                IsNull::Yes => Ok(postgres_protocol::IsNull::Yes),
            },
            out,
        )?;
        Ok(IsNull::No)
    }

    #[inline]
    fn accepts(_: &Type) -> bool {
        true
    }

    to_sql_checked!();
}
