use std::any::Any;
use std::collections::HashMap;

use async_trait::async_trait;

use super::credentials::Credentials;
use crate::exceptions::Result;

#[async_trait]
pub trait CredentialsProvider: Send + Sync {
    async fn get_credential(&self) -> Result<Box<dyn Credentials>>;

    async fn get_credential_map(&self) -> Result<HashMap<String, String>> {
        Ok(self.get_credential().await?.properties())
    }

    fn as_any(&self) -> &dyn Any;
}
