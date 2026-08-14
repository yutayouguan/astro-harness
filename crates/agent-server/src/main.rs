//! `backend` 二进制入口：调用 [`backend::run`]。

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    backend::run().await
}
