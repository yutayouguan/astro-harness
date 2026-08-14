//! `backend` 二进制入口：调用 [`server::run`]。

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    server::run().await
}
