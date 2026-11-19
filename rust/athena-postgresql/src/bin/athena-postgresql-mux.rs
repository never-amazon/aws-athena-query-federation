use athena_federation_sdk::lambda;

#[tokio::main]
async fn main() -> athena_federation_sdk::Result<()> {
    lambda::run(athena_postgresql::mux_composite_handler_from_environment().await?).await
}
