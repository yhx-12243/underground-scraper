use tokio_postgres::{Client, Statement};

#[derive(Clone, Copy)]
pub struct DBWrapper<'a, const N: usize> {
    pub conn: &'a Client,
    pub stmts: [&'a Statement; N],
}
