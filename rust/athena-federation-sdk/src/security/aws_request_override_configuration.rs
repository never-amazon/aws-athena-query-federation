use aws_credential_types::Credentials;
use indexmap::IndexMap;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AwsRequestOverrideConfiguration {
    headers: IndexMap<String, Vec<String>>,
    credentials: Option<Credentials>,
}

impl AwsRequestOverrideConfiguration {
    pub fn builder() -> AwsRequestOverrideConfigurationBuilder {
        AwsRequestOverrideConfigurationBuilder::default()
    }

    pub fn headers(&self) -> &IndexMap<String, Vec<String>> {
        &self.headers
    }

    pub fn credentials(&self) -> Option<&Credentials> {
        self.credentials.as_ref()
    }

    pub fn to_builder(&self) -> AwsRequestOverrideConfigurationBuilder {
        AwsRequestOverrideConfigurationBuilder {
            headers: self.headers.clone(),
            credentials: self.credentials.clone(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AwsRequestOverrideConfigurationBuilder {
    headers: IndexMap<String, Vec<String>>,
    credentials: Option<Credentials>,
}

impl AwsRequestOverrideConfigurationBuilder {
    #[must_use]
    pub fn put_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), vec![value.into()]);
        self
    }

    #[must_use]
    pub fn put_header_with_values(mut self, name: impl Into<String>, values: Vec<String>) -> Self {
        self.headers.insert(name.into(), values);
        self
    }

    #[must_use]
    pub fn credentials_provider(mut self, credentials: Credentials) -> Self {
        self.credentials = Some(credentials);
        self
    }

    pub fn build(self) -> AwsRequestOverrideConfiguration {
        AwsRequestOverrideConfiguration {
            headers: self.headers,
            credentials: self.credentials,
        }
    }
}

macro_rules! send_with_override {
    ($builder:expr, $override:expr, $config:ty) => {{
        let builder = $builder;
        match $override {
            None => builder.send().await,
            Some(o) => {
                let headers = o.headers().clone();
                let mut operation = builder.customize();
                if let Some(credentials) = o.credentials() {
                    operation = operation.config_override(
                        <$config>::default().credentials_provider(credentials.clone()),
                    );
                }
                operation
                    .mutate_request(move |request| {
                        for (name, values) in &headers {
                            for value in values {
                                request.headers_mut().append(name.clone(), value.clone());
                            }
                        }
                    })
                    .send()
                    .await
            }
        }
    }};
}

pub(crate) use send_with_override;
