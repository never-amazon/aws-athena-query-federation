use athena_federation_sdk::lambda;
use athena_postgresql::PostGreSqlMuxCompositeHandler;

#[tokio::main]
async fn main() -> athena_federation_sdk::Result<()> {
    lambda::run(PostGreSqlMuxCompositeHandler::from_environment().await?).await
}
