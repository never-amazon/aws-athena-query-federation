use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use athena_jdbc::sql::PreparedStatement;
use athena_snowflake::sql::{
    parse_connection_settings, SnowflakeAuth, SnowflakeDriver, SnowflakeJdbcDriver,
    TimestampTypeMapping,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;

const LOGIN_RESPONSE: &str = r#"{"success":true,"data":{"token":"session-token"}}"#;
const ABORT_RESPONSE: &str = r#"{"success":true}"#;
const CANCELLED_RESPONSE: &str = r#"{"code":"000604","sqlState":"57014","message":"SQL execution canceled","success":false,"data":{"queryId":"query-id"}}"#;
const QUERY_RESPONSE: &str = r#"{"success":true,"data":{"queryId":"query-id","rowset":[["1"]],"rowtype":[{"name":"1","nullable":false,"precision":1,"scale":0,"type":"fixed"}],"queryResultFormat":"json"}}"#;

#[derive(Default)]
struct FakeSnowflake {
    requests: Mutex<Vec<String>>,
    aborted: Notify,
    hold_queries: bool,
}

impl FakeSnowflake {
    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

async fn read_request(stream: &mut TcpStream) -> String {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let read = stream.read(&mut chunk).await.unwrap();
        assert!(read > 0, "connection closed before the request ended");
        buffer.extend_from_slice(&chunk[..read]);
        let text = String::from_utf8_lossy(&buffer);
        if let Some(header_end) = text.find("\r\n\r\n") {
            let content_length = text[..header_end]
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0);
            if buffer.len() >= header_end + 4 + content_length {
                return text.into_owned();
            }
        }
    }
}

async fn write_response(stream: &mut TcpStream, body: &str) {
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await.unwrap();
}

async fn serve(fake: Arc<FakeSnowflake>, mut stream: TcpStream) {
    let request = read_request(&mut stream).await;
    fake.requests.lock().unwrap().push(request.clone());
    if request.starts_with("POST /session/v1/login-request") {
        write_response(&mut stream, LOGIN_RESPONSE).await;
    } else if request.starts_with("POST /queries/v1/abort-request") {
        fake.aborted.notify_one();
        write_response(&mut stream, ABORT_RESPONSE).await;
    } else if request.starts_with("POST /queries/v1/query-request") {
        if fake.hold_queries {
            fake.aborted.notified().await;
            write_response(&mut stream, CANCELLED_RESPONSE).await;
        } else {
            write_response(&mut stream, QUERY_RESPONSE).await;
        }
    }
}

async fn start(fake: Arc<FakeSnowflake>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(serve(Arc::clone(&fake), stream));
        }
    });
    address
}

fn url(address: SocketAddr) -> String {
    format!("jdbc:snowflake://{address}/?ssl=off")
}

fn properties(extra: &[(&str, &str)]) -> HashMap<String, String> {
    [("user", "athena"), ("password", "secret")]
        .iter()
        .chain(extra)
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

fn body(request: &str) -> serde_json::Value {
    let (_, body) = request.split_once("\r\n\r\n").unwrap();
    serde_json::from_str(body).unwrap()
}

#[tokio::test]
async fn login_sends_json_query_result_format() {
    let fake = Arc::new(FakeSnowflake::default());
    let address = start(Arc::clone(&fake)).await;
    SnowflakeJdbcDriver::new()
        .connect(&url(address), &properties(&[]), SnowflakeAuth::Properties)
        .await
        .unwrap();
    let login = body(&fake.requests()[0]);
    assert_eq!(
        "JSON",
        login["data"]["SESSION_PARAMETERS"]["QUERY_RESULT_FORMAT"]
    );
}

#[tokio::test]
async fn login_keeps_an_explicit_query_result_format() {
    let fake = Arc::new(FakeSnowflake::default());
    let address = start(Arc::clone(&fake)).await;
    SnowflakeJdbcDriver::new()
        .connect(
            &url(address),
            &properties(&[("QUERY_RESULT_FORMAT", "ARROW")]),
            SnowflakeAuth::Properties,
        )
        .await
        .unwrap();
    let login = body(&fake.requests()[0]);
    assert_eq!(
        "ARROW",
        login["data"]["SESSION_PARAMETERS"]["QUERY_RESULT_FORMAT"]
    );
}

#[test]
fn query_response_timeout_waits_for_the_lambda_limit() {
    let settings = parse_connection_settings(
        "jdbc:snowflake://acct.snowflakecomputing.com/",
        &properties(&[]),
    )
    .unwrap();
    let config = format!("{:?}", settings.client_config(None).unwrap());
    assert!(config.contains("query_response_timeout: 900s"), "{config}");
}

#[test]
fn timestamp_type_mapping_comes_from_the_connection_string() {
    let cases = [
        ("", TimestampTypeMapping::Ltz),
        (
            "?client_timestamp_type_mapping=timestamp_ntz",
            TimestampTypeMapping::Ntz,
        ),
        (
            "?CLIENT_TIMESTAMP_TYPE_MAPPING=TIMESTAMP_LTZ",
            TimestampTypeMapping::Ltz,
        ),
    ];
    for (query, mapping) in cases {
        let settings = parse_connection_settings(
            &format!("jdbc:snowflake://acct.snowflakecomputing.com/{query}"),
            &properties(&[]),
        )
        .unwrap();
        assert_eq!(mapping, settings.timestamp_mapping, "{query}");
        assert_eq!(
            !query.is_empty(),
            settings
                .session_parameters
                .iter()
                .any(|(key, _)| key == "CLIENT_TIMESTAMP_TYPE_MAPPING")
        );
    }
}

#[test]
fn invalid_query_timeout_property_fails_like_the_jdbc_driver() {
    let error = parse_connection_settings(
        "jdbc:snowflake://acct.snowflakecomputing.com/?queryTimeout=abc",
        &properties(&[]),
    )
    .unwrap_err();
    assert_eq!(
        "Invalid parameter value java.lang.String for parameter type java.lang.Integer.",
        error.message()
    );
    let settings = parse_connection_settings(
        "jdbc:snowflake://acct.snowflakecomputing.com/?queryTimeout=30",
        &properties(&[]),
    )
    .unwrap();
    assert_eq!(Some(Duration::from_secs(30)), settings.query_timeout);
}

#[tokio::test]
async fn statement_query_timeout_cancels_the_query_in_snowflake() {
    let fake = Arc::new(FakeSnowflake {
        hold_queries: true,
        ..FakeSnowflake::default()
    });
    let address = start(Arc::clone(&fake)).await;
    let connection = SnowflakeJdbcDriver::new()
        .connect(&url(address), &properties(&[]), SnowflakeAuth::Properties)
        .await
        .unwrap();
    let mut statement = PreparedStatement::new("select 1");
    statement.set_query_timeout(std::time::Duration::from_secs(1));
    let error = connection.execute_query(&statement).await.unwrap_err();
    assert_eq!("SQL execution canceled", error.message());
    assert_eq!(604, error.error_code());
    assert!(fake
        .requests()
        .iter()
        .any(|request| request.starts_with("POST /queries/v1/abort-request")));
}

#[tokio::test]
async fn connection_query_timeout_property_cancels_the_query_in_snowflake() {
    let fake = Arc::new(FakeSnowflake {
        hold_queries: true,
        ..FakeSnowflake::default()
    });
    let address = start(Arc::clone(&fake)).await;
    let connection = SnowflakeJdbcDriver::new()
        .connect(
            &url(address),
            &properties(&[("queryTimeout", "1")]),
            SnowflakeAuth::Properties,
        )
        .await
        .unwrap();
    let error = connection
        .execute_query(&PreparedStatement::new("select 1"))
        .await
        .unwrap_err();
    assert_eq!("SQL execution canceled", error.message());
    assert!(fake
        .requests()
        .iter()
        .any(|request| request.starts_with("POST /queries/v1/abort-request")));
}

#[tokio::test]
async fn query_without_timeout_returns_rows() {
    let fake = Arc::new(FakeSnowflake::default());
    let address = start(Arc::clone(&fake)).await;
    let connection = SnowflakeJdbcDriver::new()
        .connect(&url(address), &properties(&[]), SnowflakeAuth::Properties)
        .await
        .unwrap();
    let mut result = connection
        .execute_query(&PreparedStatement::new("select 1"))
        .await
        .unwrap();
    let result = result.next_row().await.unwrap().unwrap();
    assert_eq!(Some(1), result.value(0).unwrap().to_long().unwrap());
    assert!(!fake
        .requests()
        .iter()
        .any(|request| request.starts_with("POST /queries/v1/abort-request")));
}
