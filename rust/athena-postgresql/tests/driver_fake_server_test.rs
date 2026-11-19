use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use athena_jdbc::sql::{JdbcConnection, SqlError};
use athena_postgresql::PgConnection;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const SSL_REQUEST: i32 = 80_877_103;
const CANCEL_REQUEST: i32 = 80_877_102;

type Log = Arc<Mutex<Vec<String>>>;

fn log(events: &Log, event: impl Into<String>) {
    events.lock().unwrap().push(event.into());
}

fn events(events: &Log) -> Vec<String> {
    events.lock().unwrap().clone()
}

fn msg(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    out.extend_from_slice(&i32::try_from(body.len() + 4).unwrap().to_be_bytes());
    out.extend_from_slice(body);
    out
}

fn auth(code: i32, data: &[u8]) -> Vec<u8> {
    msg(b'R', &[&code.to_be_bytes()[..], data].concat())
}

fn ready() -> Vec<u8> {
    msg(b'Z', b"I")
}

fn error(state: &str, text: &str) -> Vec<u8> {
    msg(b'E', format!("SFATAL\0C{state}\0M{text}\0\0").as_bytes())
}

async fn read_startup(stream: &mut TcpStream) -> Option<(i32, Vec<u8>)> {
    let len = stream.read_i32().await.ok()?;
    let mut body = vec![0; usize::try_from(len - 4).ok()?];
    stream.read_exact(&mut body).await.ok()?;
    let code = i32::from_be_bytes([body[0], body[1], body[2], body[3]]);
    Some((code, body[4..].to_vec()))
}

async fn read_message(stream: &mut TcpStream) -> Option<(u8, Vec<u8>)> {
    let tag = stream.read_u8().await.ok()?;
    let len = stream.read_i32().await.ok()?;
    let mut body = vec![0; usize::try_from(len - 4).ok()?];
    stream.read_exact(&mut body).await.ok()?;
    Some((tag, body))
}

async fn plain_startup(stream: &mut TcpStream) -> Option<Vec<u8>> {
    let (code, body) = read_startup(stream).await?;
    if code == SSL_REQUEST {
        stream.write_all(b"N").await.ok()?;
        return read_startup(stream).await.map(|(_, body)| body);
    }
    Some(body)
}

struct Fake {
    port: u16,
    events: Log,
}

impl Fake {
    fn url(&self, query: &str) -> String {
        format!(
            "jdbc:postgresql://127.0.0.1:{}/db?user=u&password=secret&{query}",
            self.port
        )
    }
}

async fn fake<F, Fut>(handler: F) -> Fake
where
    F: Fn(TcpStream, Log, usize) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let events: Log = Arc::default();
    let shared = Arc::clone(&events);
    tokio::spawn(async move {
        let mut n = 0;
        while let Ok((stream, _)) = listener.accept().await {
            n += 1;
            log(&shared, format!("connection {n}"));
            tokio::spawn(handler(stream, Arc::clone(&shared), n));
        }
    });
    Fake { port, events }
}

async fn connect(url: &str) -> Result<PgConnection, SqlError> {
    tokio::time::timeout(
        Duration::from_secs(20),
        PgConnection::connect(url, &HashMap::new()),
    )
    .await
    .expect("connect must not hang")
}

fn err(result: Result<PgConnection, SqlError>) -> SqlError {
    match result {
        Ok(_) => panic!("expected the connection to fail"),
        Err(e) => e,
    }
}

async fn settle() {
    tokio::time::sleep(Duration::from_millis(300)).await;
}

#[tokio::test]
async fn ssl_error_reply_never_falls_back_to_plaintext_when_encryption_is_required() {
    for mode in ["require", "verify-ca", "verify-full", "REQUIRE"] {
        let server = fake(|mut stream, events, _| async move {
            if let Some((code, body)) = read_startup(&mut stream).await {
                if code == SSL_REQUEST {
                    let _ = stream.write_all(b"E").await;
                } else {
                    log(&events, format!("plaintext startup {}", body.len()));
                }
            }
        })
        .await;
        let error = err(connect(&server.url(&format!("sslmode={mode}"))).await);
        settle().await;
        assert_eq!(
            error.message(),
            "The server does not support SSL.",
            "{mode}"
        );
        assert_eq!(error.sql_state(), Some("08004"));
        assert_eq!(events(&server.events), vec!["connection 1"], "{mode}");
    }
}

#[tokio::test]
async fn ssl_error_reply_reconnects_in_plaintext_only_under_prefer() {
    let server = fake(|mut stream, events, n| async move {
        if n == 1 {
            read_startup(&mut stream).await;
            let _ = stream.write_all(b"E").await;
            return;
        }
        let (code, _) = read_startup(&mut stream).await.unwrap();
        log(&events, format!("second startup code {code}"));
        let _ = stream.write_all(&[auth(0, b""), ready()].concat()).await;
        tokio::time::sleep(Duration::from_secs(1)).await;
    })
    .await;
    connect(&server.url("sslmode=prefer")).await.unwrap();
    assert_eq!(
        events(&server.events),
        vec!["connection 1", "connection 2", "second startup code 196608"]
    );
}

#[tokio::test]
async fn prefer_does_not_retry_in_plaintext_after_a_tls_failure() {
    let server = fake(|mut stream, events, n| async move {
        let (code, _) = read_startup(&mut stream).await.unwrap_or_default();
        if n == 1 && code == SSL_REQUEST {
            let _ = stream.write_all(b"S").await;
            let mut hello = [0; 512];
            let _ = stream.read(&mut hello).await;
            let _ = stream
                .write_all(&[0x15, 0x03, 0x03, 0x00, 0x02, 0x02, 0x28])
                .await;
        } else {
            log(&events, format!("retry startup code {code}"));
        }
    })
    .await;
    let error = err(connect(&server.url("sslmode=prefer")).await);
    settle().await;
    assert!(
        error.message().starts_with("SSL error: "),
        "{}",
        error.message()
    );
    assert_eq!(error.sql_state(), Some("08006"));
    assert_eq!(events(&server.events), vec!["connection 1"]);
}

#[tokio::test]
async fn prefer_retries_without_ssl_after_28000_and_after_an_ssl_response_timeout() {
    let server = fake(|mut stream, events, n| async move {
        if n == 1 {
            plain_startup(&mut stream).await;
            let _ = stream
                .write_all(&error("28000", "no pg_hba.conf entry"))
                .await;
            return;
        }
        let (code, _) = read_startup(&mut stream).await.unwrap();
        log(&events, format!("retry startup code {code}"));
        let _ = stream.write_all(&[auth(0, b""), ready()].concat()).await;
        tokio::time::sleep(Duration::from_secs(1)).await;
    })
    .await;
    connect(&server.url("sslmode=prefer")).await.unwrap();
    assert_eq!(events(&server.events)[2], "retry startup code 196608");

    let silent = fake(|mut stream, events, n| async move {
        let (code, _) = read_startup(&mut stream).await.unwrap();
        if n == 1 {
            tokio::time::sleep(Duration::from_secs(5)).await;
            return;
        }
        log(&events, format!("retry startup code {code}"));
        let _ = stream.write_all(&[auth(0, b""), ready()].concat()).await;
        tokio::time::sleep(Duration::from_secs(1)).await;
    })
    .await;
    let started = Instant::now();
    connect(&silent.url("sslmode=prefer&sslResponseTimeout=200"))
        .await
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(events(&silent.events)[2], "retry startup code 196608");

    let silent_require = fake(|mut stream, _, _| async move {
        read_startup(&mut stream).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
    })
    .await;
    let error = err(connect(&silent_require.url("sslmode=require&sslResponseTimeout=200")).await);
    assert_eq!(error.message(), "The connection attempt failed.");
    assert_eq!(events(&silent_require.events), vec!["connection 1"]);
}

#[tokio::test]
async fn hostile_frame_lengths_fail_without_allocating_or_panicking() {
    for len in [i32::MIN, 3, 0x7FFF_FFF0, (1 << 30) + 5] {
        let server = fake(move |mut stream, _, _| async move {
            plain_startup(&mut stream).await;
            let mut frame = vec![b'R'];
            frame.extend_from_slice(&len.to_be_bytes());
            let _ = stream.write_all(&frame).await;
            tokio::time::sleep(Duration::from_secs(5)).await;
        })
        .await;
        let started = Instant::now();
        let error = err(connect(&server.url("sslmode=disable")).await);
        assert!(started.elapsed() < Duration::from_secs(2), "{len}");
        assert_eq!(error.message(), "The connection attempt failed.", "{len}");
    }
}

fn row_description(oid: u32) -> Vec<u8> {
    let mut body = 1i16.to_be_bytes().to_vec();
    body.extend_from_slice(b"x\0");
    body.extend_from_slice(&0u32.to_be_bytes());
    body.extend_from_slice(&0i16.to_be_bytes());
    body.extend_from_slice(&oid.to_be_bytes());
    body.extend_from_slice(&(-1i16).to_be_bytes());
    body.extend_from_slice(&(-1i32).to_be_bytes());
    body.extend_from_slice(&0i16.to_be_bytes());
    msg(b'T', &body)
}

fn data_row(value: Option<&[u8]>) -> Vec<u8> {
    let mut body = 1i16.to_be_bytes().to_vec();
    match value {
        Some(v) => {
            body.extend_from_slice(&i32::try_from(v.len()).unwrap().to_be_bytes());
            body.extend_from_slice(v);
        }
        None => body.extend_from_slice(&(-1i32).to_be_bytes()),
    }
    msg(b'D', &body)
}

async fn until(stream: &mut TcpStream, tag: u8, seen: &Log) -> Option<Vec<u8>> {
    loop {
        let (t, body) = read_message(stream).await?;
        log(seen, format!("{}", char::from(t)));
        if t == tag {
            return Some(body);
        }
    }
}

#[tokio::test]
async fn deeply_nested_arrays_do_not_overflow_the_stack() {
    let server = fake(|mut stream, events, _| async move {
        plain_startup(&mut stream).await;
        let _ = stream.write_all(&[auth(0, b""), ready()].concat()).await;
        until(&mut stream, b'H', &events).await;
        let _ = stream
            .write_all(&[msg(b'1', b""), msg(b'2', b""), row_description(1007)].concat())
            .await;
        until(&mut stream, b'S', &events).await;
        let mut rows = Vec::new();
        for depth in [6usize, 7, 200_000] {
            let value = [vec![b'{'; depth], b"1".to_vec(), vec![b'}'; depth]].concat();
            rows.extend(data_row(Some(&value)));
        }
        rows.extend(msg(b'C', b"SELECT 3\0"));
        rows.extend(ready());
        let _ = stream.write_all(&rows).await;
        tokio::time::sleep(Duration::from_secs(2)).await;
    })
    .await;
    let connection = connect(&server.url("sslmode=disable")).await.unwrap();
    let statement = connection.prepare_statement("select x").await.unwrap();
    let mut rows = connection.execute_query(&statement).await.unwrap();
    assert!(rows.next().await.unwrap());
    assert!(rows
        .current_row()
        .unwrap()
        .get_array(1usize)
        .unwrap()
        .is_some());
    assert!(rows.next().await.unwrap());
    assert!(rows.current_row().unwrap().get_array(1usize).is_err());
    assert!(rows.next().await.unwrap());
    assert!(rows.current_row().unwrap().get_array(1usize).is_err());
    assert!(!rows.next().await.unwrap());
}

fn sasl_mechanisms(names: &[&str]) -> Vec<u8> {
    let mut data = Vec::new();
    for name in names {
        data.extend_from_slice(name.as_bytes());
        data.push(0);
    }
    data.push(0);
    data
}

async fn scram_server(stream: &mut TcpStream, iterations: &str) -> Option<()> {
    plain_startup(stream).await?;
    stream
        .write_all(&auth(10, &sasl_mechanisms(&["SCRAM-SHA-256"])))
        .await
        .ok()?;
    let (_, body) = read_message(stream).await?;
    let text = String::from_utf8_lossy(&body).into_owned();
    let nonce = text.split("r=").nth(1)?.to_string();
    let first = format!("r={nonce}SERVER,s=c2FsdHNhbHQ=,i={iterations}");
    stream.write_all(&auth(11, first.as_bytes())).await.ok()
}

#[tokio::test]
async fn scram_rejects_zero_and_excessive_iteration_counts_before_hashing() {
    for (iterations, query, expected) in [
        ("0", "", "SCRAM authentication failed: 'iterationCount' must be positive, was: 0"),
        (
            "100001",
            "",
            "Server requested 100,001 SCRAM PBKDF2 iterations, which exceeds the client-side limit of 100,000. If you trust this server, raise the scramMaxIterations connection property.",
        ),
        (
            "5000",
            "scramMaxIterations=4096",
            "Server requested 5,000 SCRAM PBKDF2 iterations, which exceeds the client-side limit of 4,096. If you trust this server, raise the scramMaxIterations connection property.",
        ),
    ] {
        let server = fake(move |mut stream, events, _| async move {
            scram_server(&mut stream, iterations).await;
            if read_message(&mut stream).await.is_some() {
                log(&events, "client-final sent");
            }
        })
        .await;
        let started = Instant::now();
        let error = err(connect(&server.url(&format!("sslmode=disable&{query}"))).await);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(error.message(), expected);
        assert_eq!(error.sql_state(), Some("08004"));
        settle().await;
        assert_eq!(events(&server.events), vec!["connection 1"]);
    }
}

#[tokio::test]
async fn scram_requires_a_valid_server_signature() {
    let skipped = fake(|mut stream, _, _| async move {
        scram_server(&mut stream, "4096").await;
        read_message(&mut stream).await;
        let _ = stream.write_all(&[auth(0, b""), ready()].concat()).await;
        tokio::time::sleep(Duration::from_secs(1)).await;
    })
    .await;
    let error = err(connect(&skipped.url("sslmode=disable")).await);
    assert_eq!(
        error.message(),
        "SCRAM authentication failed: the server sent AuthenticationOk before the SCRAM exchange finished"
    );

    let forged = fake(|mut stream, _, _| async move {
        scram_server(&mut stream, "4096").await;
        read_message(&mut stream).await;
        let final_message = b"v=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        let _ = stream
            .write_all(&[auth(12, final_message), auth(0, b""), ready()].concat())
            .await;
        tokio::time::sleep(Duration::from_secs(1)).await;
    })
    .await;
    let error = err(connect(&forged.url("sslmode=disable")).await);
    assert_eq!(
        error.message(),
        "SCRAM authentication failed: Invalid SCRAM server signature"
    );
    assert_eq!(error.sql_state(), Some("08004"));
}

#[tokio::test]
async fn require_auth_and_channel_binding_are_enforced() {
    let cleartext = || {
        fake(|mut stream, events, _| async move {
            plain_startup(&mut stream).await;
            let _ = stream.write_all(&auth(3, b"")).await;
            if let Some((tag, _)) = read_message(&mut stream).await {
                log(&events, format!("password message {}", char::from(tag)));
            }
        })
    };
    let server = cleartext().await;
    let error = err(connect(&server.url("sslmode=disable&requireAuth=scram-sha-256")).await);
    settle().await;
    assert_eq!(
        error.message(),
        "Authentication method is not allowed by requireAuth"
    );
    assert_eq!(events(&server.events), vec!["connection 1"]);

    let server = cleartext().await;
    let error = err(connect(&server.url("sslmode=disable&channelBinding=require")).await);
    settle().await;
    assert_eq!(
        error.message(),
        "Channel binding is required, but server requested 'password' authentication. Channel binding is only supported with SCRAM authentication over encrypted connections."
    );
    assert_eq!(events(&server.events), vec!["connection 1"]);

    let trust = fake(|mut stream, _, _| async move {
        plain_startup(&mut stream).await;
        let _ = stream.write_all(&[auth(0, b""), ready()].concat()).await;
        tokio::time::sleep(Duration::from_secs(1)).await;
    })
    .await;
    let error = err(connect(&trust.url("sslmode=disable&requireAuth=password")).await);
    assert_eq!(
        error.message(),
        "Authentication method is not allowed by requireAuth"
    );
    let error = err(connect(&trust.url("sslmode=disable&channelBinding=require")).await);
    assert!(error
        .message()
        .starts_with("Channel binding is required, but server skipped authentication."));
    let error = err(connect(&trust.url("sslmode=disable&requireAuth=md5,!password")).await);
    assert_eq!(
        error.message(),
        "requireAuth cannot mix positive and negative authentication methods"
    );
    connect(&trust.url("sslmode=disable&requireAuth=none"))
        .await
        .unwrap();

    let scram_plus = fake(|mut stream, _, _| async move {
        plain_startup(&mut stream).await;
        let _ = stream
            .write_all(&auth(
                10,
                &sasl_mechanisms(&["SCRAM-SHA-256-PLUS", "SCRAM-SHA-256"]),
            ))
            .await;
        tokio::time::sleep(Duration::from_secs(1)).await;
    })
    .await;
    let error = err(connect(&scram_plus.url("sslmode=disable&channelBinding=require")).await);
    assert_eq!(
        error.message(),
        "Channel Binding is required, but SSL is not in use"
    );
}

#[tokio::test]
async fn login_timeout_bounds_the_whole_connection_attempt() {
    let server = fake(|mut stream, _, _| async move {
        plain_startup(&mut stream).await;
        tokio::time::sleep(Duration::from_secs(10)).await;
    })
    .await;
    let started = Instant::now();
    let error = err(connect(&server.url("sslmode=disable&loginTimeout=0.5")).await);
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(error.message(), "Connection attempt timed out.");
    assert_eq!(error.sql_state(), Some("08001"));
}

async fn simple_session(stream: &mut TcpStream) {
    plain_startup(stream).await;
    let key = [7i32.to_be_bytes(), 99i32.to_be_bytes()].concat();
    let _ = stream
        .write_all(&[auth(0, b""), msg(b'K', &key), ready()].concat())
        .await;
}

#[tokio::test]
async fn fetch_size_reads_through_a_portal_and_commit_does_not_drain_it() {
    let server = fake(|mut stream, events, _| async move {
        simple_session(&mut stream).await;
        let begin = until(&mut stream, b'Q', &events).await.unwrap();
        log(
            &events,
            String::from_utf8_lossy(&begin)
                .trim_end_matches('\0')
                .to_string(),
        );
        let _ = stream
            .write_all(&[msg(b'C', b"BEGIN\0"), msg(b'Z', b"T")].concat())
            .await;
        until(&mut stream, b'H', &events).await;
        let _ = stream
            .write_all(&[msg(b'1', b""), msg(b'2', b""), row_description(23)].concat())
            .await;
        let execute = until(&mut stream, b'E', &events).await.unwrap();
        let rows = i32::from_be_bytes(execute[execute.len() - 4..].try_into().unwrap());
        log(&events, format!("execute max_rows {rows}"));
        until(&mut stream, b'S', &events).await;
        let mut batch = Vec::new();
        for i in 0..rows {
            batch.extend(data_row(Some(i.to_string().as_bytes())));
        }
        batch.extend(msg(b's', b""));
        batch.extend(msg(b'Z', b"T"));
        let _ = stream.write_all(&batch).await;
        let commit = until(&mut stream, b'Q', &events).await.unwrap();
        log(
            &events,
            String::from_utf8_lossy(&commit)
                .trim_end_matches('\0')
                .to_string(),
        );
        let _ = stream
            .write_all(&[msg(b'C', b"COMMIT\0"), ready()].concat())
            .await;
        tokio::time::sleep(Duration::from_secs(1)).await;
    })
    .await;
    let connection = connect(&server.url("sslmode=disable")).await.unwrap();
    connection.set_auto_commit(false).await.unwrap();
    let mut statement = connection.prepare_statement("select x").await.unwrap();
    statement.set_fetch_size(5);
    let mut rows = connection.execute_query(&statement).await.unwrap();
    assert!(rows.next().await.unwrap());
    connection.commit().await.unwrap();
    let seen = events(&server.events);
    assert!(seen.contains(&"execute max_rows 5".to_string()), "{seen:?}");
    assert_eq!(seen.last().unwrap(), "COMMIT");
    assert_eq!(seen.iter().filter(|e| *e == "E").count(), 1, "{seen:?}");
}

#[tokio::test]
async fn dropping_a_read_mid_row_keeps_the_connection_in_sync() {
    let server = fake(|mut stream, events, _| async move {
        simple_session(&mut stream).await;
        until(&mut stream, b'H', &events).await;
        let _ = stream
            .write_all(&[msg(b'1', b""), msg(b'2', b""), row_description(25)].concat())
            .await;
        until(&mut stream, b'S', &events).await;
        let row = data_row(Some(b"first"));
        let _ = stream.write_all(&row[..4]).await;
        tokio::time::sleep(Duration::from_millis(400)).await;
        let rest = [&row[4..], &msg(b'C', b"SELECT 1\0"), &ready()].concat();
        let _ = stream.write_all(&rest).await;
        until(&mut stream, b'H', &events).await;
        let _ = stream
            .write_all(&[msg(b'1', b""), msg(b'2', b""), row_description(25)].concat())
            .await;
        until(&mut stream, b'S', &events).await;
        let _ = stream
            .write_all(&[data_row(Some(b"42")), msg(b'C', b"SELECT 1\0"), ready()].concat())
            .await;
        tokio::time::sleep(Duration::from_secs(1)).await;
    })
    .await;
    let connection = connect(&server.url("sslmode=disable")).await.unwrap();
    let statement = connection.prepare_statement("select x").await.unwrap();
    let mut rows = connection.execute_query(&statement).await.unwrap();
    let first = tokio::time::timeout(Duration::from_millis(100), rows.next()).await;
    assert!(first.is_err());
    drop(rows);
    let second = connection.prepare_statement("select 42").await.unwrap();
    let mut rows = tokio::time::timeout(Duration::from_secs(5), connection.execute_query(&second))
        .await
        .unwrap()
        .unwrap();
    assert!(rows.next().await.unwrap());
    assert_eq!(
        rows.current_row()
            .unwrap()
            .get_string(1usize)
            .unwrap()
            .as_deref(),
        Some("42")
    );
}

#[tokio::test]
async fn query_timeout_sends_a_cancel_request_with_the_backend_key() {
    let server = fake(|mut stream, events, n| async move {
        if n > 1 {
            if let Some((code, body)) = read_startup(&mut stream).await {
                let pid = i32::from_be_bytes(body[0..4].try_into().unwrap());
                let key = i32::from_be_bytes(body[4..8].try_into().unwrap());
                log(&events, format!("cancel code {code} pid {pid} key {key}"));
            }
            return;
        }
        simple_session(&mut stream).await;
        until(&mut stream, b'H', &events).await;
        let _ = stream
            .write_all(&[msg(b'1', b""), msg(b'2', b""), row_description(25)].concat())
            .await;
        until(&mut stream, b'S', &events).await;
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let _ = stream
            .write_all(
                &[
                    error("57014", "canceling statement due to user request"),
                    ready(),
                ]
                .concat(),
            )
            .await;
        tokio::time::sleep(Duration::from_secs(1)).await;
    })
    .await;
    let connection = connect(&server.url("sslmode=disable")).await.unwrap();
    let mut statement = connection
        .prepare_statement("select pg_sleep(5)")
        .await
        .unwrap();
    statement.set_query_timeout(std::time::Duration::from_secs(1));
    let mut rows = connection.execute_query(&statement).await.unwrap();
    let error = rows.next().await.unwrap_err();
    assert_eq!(error.sql_state(), Some("57014"));
    let seen = events(&server.events);
    assert!(
        seen.contains(&format!("cancel code {CANCEL_REQUEST} pid 7 key 99")),
        "{seen:?}"
    );
}
