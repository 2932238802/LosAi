#[tokio::main]
async fn main() -> anyhow::Result<()> {
    lostoken_api::run().await
}
