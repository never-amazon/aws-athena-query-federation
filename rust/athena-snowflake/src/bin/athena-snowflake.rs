use athena_federation_sdk::lambda;
use athena_snowflake::SnowflakeCompositeHandler;

#[tokio::main]
async fn main() -> athena_federation_sdk::Result<()> {
    lambda::run(SnowflakeCompositeHandler::from_environment().await?).await
}
