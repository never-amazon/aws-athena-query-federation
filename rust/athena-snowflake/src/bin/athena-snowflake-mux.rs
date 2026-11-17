use athena_federation_sdk::lambda;
use athena_snowflake::SnowflakeMuxCompositeHandler;

#[tokio::main]
async fn main() -> athena_federation_sdk::Result<()> {
    lambda::run(SnowflakeMuxCompositeHandler::from_environment().await?).await
}
