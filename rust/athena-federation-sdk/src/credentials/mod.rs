#[allow(
    clippy::module_inception,
    reason = "module names mirror the Java package and class"
)]
mod credentials;
pub mod credentials_constants;
mod credentials_provider;
mod credentials_provider_factory;
mod cross_account_credentials_provider;
mod default_credentials;
mod default_credentials_provider;
mod http_client;
mod oauth_access_token_credentials;
mod oauth_credentials_provider;
mod static_credentials_provider;

pub use credentials::Credentials;
pub use credentials_provider::CredentialsProvider;
pub use credentials_provider_factory::create_credential_provider;
pub use cross_account_credentials_provider::get_cross_account_credentials_if_present;
pub use default_credentials::DefaultCredentials;
pub use default_credentials_provider::DefaultCredentialsProvider;
pub use http_client::{
    HttpClient, HttpClientError, HttpRequest, HttpRequestBuilder, HttpResponse, ReqwestHttpClient,
};
pub use oauth_access_token_credentials::OAuthAccessTokenCredentials;
pub use oauth_credentials_provider::{OAuthCredentialsProvider, OAuthFlow};
pub use static_credentials_provider::StaticCredentialsProvider;
