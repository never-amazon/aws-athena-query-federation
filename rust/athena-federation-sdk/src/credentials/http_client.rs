use std::fmt;
use std::time::Duration;

use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    method: String,
    uri: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
    timeout: Option<Duration>,
}

impl HttpRequest {
    pub fn builder(uri: impl Into<String>) -> HttpRequestBuilder {
        HttpRequestBuilder {
            method: "GET".to_string(),
            uri: uri.into(),
            headers: Vec::new(),
            body: None,
            timeout: None,
        }
    }

    pub fn method(&self) -> &str {
        &self.method
    }

    pub fn uri(&self) -> &str {
        &self.uri
    }

    pub fn headers(&self) -> &[(String, String)] {
        &self.headers
    }

    pub fn body(&self) -> Option<&str> {
        self.body.as_deref()
    }

    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }
}

#[derive(Debug, Clone)]
pub struct HttpRequestBuilder {
    method: String,
    uri: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
    timeout: Option<Duration>,
}

impl HttpRequestBuilder {
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn get(mut self) -> Self {
        self.method = "GET".to_string();
        self.body = None;
        self
    }

    pub fn post(mut self, body: impl Into<String>) -> Self {
        self.method = "POST".to_string();
        self.body = Some(body.into());
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn build(self) -> HttpRequest {
        HttpRequest {
            method: self.method,
            uri: self.uri,
            headers: self.headers,
            body: self.body,
            timeout: self.timeout,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    status_code: u16,
    body: String,
}

impl HttpResponse {
    pub fn new(status_code: u16, body: impl Into<String>) -> HttpResponse {
        HttpResponse {
            status_code,
            body: body.into(),
        }
    }

    pub fn status_code(&self) -> u16 {
        self.status_code
    }

    pub fn body(&self) -> &str {
        &self.body
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpClientError {
    Io(String),
    Interrupted(String),
}

impl fmt::Display for HttpClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HttpClientError::Io(m) | HttpClientError::Interrupted(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for HttpClientError {}

#[async_trait]
pub trait HttpClient: Send + Sync {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, HttpClientError>;
}

#[derive(Debug, Clone, Default)]
pub struct ReqwestHttpClient {
    client: reqwest::Client,
}

impl ReqwestHttpClient {
    pub fn new() -> ReqwestHttpClient {
        ReqwestHttpClient::default()
    }
}

#[async_trait]
impl HttpClient for ReqwestHttpClient {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, HttpClientError> {
        let method = reqwest::Method::from_bytes(request.method.as_bytes())
            .map_err(|e| HttpClientError::Io(e.to_string()))?;
        let mut builder = self.client.request(method, &request.uri);
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        if let Some(body) = request.body {
            builder = builder.body(body);
        }
        if let Some(timeout) = request.timeout {
            builder = builder.timeout(timeout);
        }
        let response = builder.send().await.map_err(|e| HttpClientError::Io(e.to_string()))?;
        let status_code = response.status().as_u16();
        let body = response.text().await.map_err(|e| HttpClientError::Io(e.to_string()))?;
        Ok(HttpResponse::new(status_code, body))
    }
}
