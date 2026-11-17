use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use super::credentials_provider::CredentialsProvider;
use crate::exceptions::Result;
use crate::security::CachableSecretsManager;

pub trait InitializableCredentialsProvider: CredentialsProvider {
    fn initialize(
        &mut self,
        secret_name: &str,
        secret_map: HashMap<String, String>,
        secrets_manager: Arc<CachableSecretsManager>,
    ) -> Result<()>;

    fn into_any(self: Box<Self>) -> Box<dyn Any>;

    fn into_credentials_provider(self: Box<Self>) -> Box<dyn CredentialsProvider>;
}
