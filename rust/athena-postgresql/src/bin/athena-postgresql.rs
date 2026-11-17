use athena_federation_sdk::lambda;
use athena_postgresql::PostGreSqlCompositeHandler;

#[tokio::main]
async fn main() -> athena_federation_sdk::Result<()> {
    lambda::run(PostGreSqlCompositeHandler::from_environment().await?).await
}
