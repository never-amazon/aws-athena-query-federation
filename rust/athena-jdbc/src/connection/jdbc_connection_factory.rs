use async_trait::async_trait;
use athena_federation_sdk::credentials::CredentialsProvider;
use athena_federation_sdk::Result;

use crate::sql::JdbcConnection;

#[async_trait]
pub trait JdbcConnectionFactory: Send + Sync {
    async fn get_connection(
        &self,
        credentials_provider: Option<&dyn CredentialsProvider>,
    ) -> Result<Box<dyn JdbcConnection>>;

    async fn close(&self) -> Result<()> {
        Ok(())
    }
}
