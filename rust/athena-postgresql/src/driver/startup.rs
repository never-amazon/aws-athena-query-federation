use std::io;
use std::time::{Duration, Instant};

use athena_jdbc::sql::SqlError;
use bytes::BytesMut;
use postgres_protocol::message::frontend;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

use super::auth::{authenticate, StartupError};
use super::error::{
    psql, psql_with_cause, CONNECTION_FAILURE, CONNECTION_REJECTED, CONNECTION_UNABLE_TO_CONNECT,
    INVALID_AUTHORIZATION, PROTOCOL_VIOLATION,
};
use super::session::{BackendKey, Session, SessionOptions};
use super::tls::{client_config, server_name, verify_peer_name};
use super::url::{ConnectSettings, HostSpec, SslMode, SslNegotiation};
use super::wire::{PgSocket, Wire};

fn timed_out(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, what.to_string())
}

async fn within<T>(
    limit: Option<Duration>,
    what: &str,
    future: impl std::future::Future<Output = io::Result<T>>,
) -> io::Result<T> {
    match limit {
        Some(limit) => tokio::time::timeout(limit, future)
            .await
            .unwrap_or_else(|_| Err(timed_out(what))),
        None => future.await,
    }
}

fn remaining(limit: Option<Duration>, started: Instant) -> Option<Duration> {
    limit.map(|limit| {
        limit
            .saturating_sub(started.elapsed())
            .max(Duration::from_millis(1))
    })
}

async fn open_tcp(
    host: &HostSpec,
    connect_timeout: Option<Duration>,
) -> Result<TcpStream, StartupError> {
    let stream = within(
        connect_timeout,
        "connect timed out",
        TcpStream::connect((host.host.as_str(), host.port)),
    )
    .await?;
    stream.set_nodelay(true)?;
    Ok(stream)
}

fn ssl_response_timeout(settings: &ConnectSettings) -> Option<Duration> {
    match (settings.ssl_response_timeout, settings.socket_timeout) {
        (Some(ssl), Some(socket)) => Some(ssl.min(socket)),
        (ssl, socket) => ssl.or(socket),
    }
}

async fn start_tls(
    stream: TcpStream,
    host: &HostSpec,
    settings: &ConnectSettings,
) -> Result<PgSocket, StartupError> {
    let config = client_config(settings).await?;
    let name = server_name(&host.host)?;
    let tls = within(
        settings.connect_timeout,
        "Read timed out",
        TlsConnector::from(config).connect(name, stream),
    )
    .await
    .map_err(|e| psql_with_cause(format!("SSL error: {e}"), CONNECTION_FAILURE, e))?;
    if settings.ssl_mode.verify_peer_name() {
        let end_entity = tls
            .get_ref()
            .1
            .peer_certificates()
            .and_then(|certs| certs.first())
            .map(AsRef::as_ref);
        verify_peer_name(&host.host, end_entity)?;
    }
    Ok(PgSocket::Tls(Box::new(tls)))
}

async fn enable_ssl(
    mut stream: TcpStream,
    host: &HostSpec,
    mode: SslMode,
    settings: &ConnectSettings,
    started: Instant,
) -> Result<PgSocket, StartupError> {
    if matches!(mode, SslMode::Disable | SslMode::Allow) {
        return Ok(PgSocket::Plain(stream));
    }
    if settings.ssl_negotiation == SslNegotiation::Direct {
        return start_tls(stream, host, settings).await;
    }
    let mut request = BytesMut::new();
    frontend::ssl_request(&mut request);
    stream.write_all(&request).await?;
    let answer = within(
        ssl_response_timeout(settings),
        "Read timed out",
        stream.read_u8(),
    )
    .await?;
    match answer {
        b'S' => start_tls(stream, host, settings).await,
        b'N' | b'E' if mode.require_encryption() => Err(StartupError::Sql(psql(
            "The server does not support SSL.",
            CONNECTION_REJECTED,
        ))),
        b'N' => Ok(PgSocket::Plain(stream)),
        b'E' => {
            drop(stream);
            Ok(PgSocket::Plain(
                open_tcp(host, remaining(settings.connect_timeout, started)).await?,
            ))
        }
        _ => Err(StartupError::Sql(psql(
            "An error occurred while setting up the SSL connection.",
            PROTOCOL_VIOLATION,
        ))),
    }
}

fn session_options(settings: &ConnectSettings) -> SessionOptions {
    SessionOptions {
        string_type_unspecified: settings.string_type_unspecified,
        log_server_error_detail: settings.log_server_error_detail,
        socket_timeout: settings.socket_timeout,
    }
}

async fn try_connect(
    host: &HostSpec,
    mode: SslMode,
    settings: &ConnectSettings,
    started: Instant,
) -> Result<Session, StartupError> {
    let user = settings.user.clone().unwrap_or_default();
    let stream = open_tcp(host, remaining(settings.connect_timeout, started)).await?;
    let socket = enable_ssl(stream, host, mode, settings, started).await?;
    let mut session = Session::new(Wire::new(socket), session_options(settings));
    let mut params: Vec<(&str, &str)> = vec![
        ("user", user.as_str()),
        ("database", settings.database.as_str()),
        ("client_encoding", "UTF8"),
        ("DateStyle", "ISO"),
        ("TimeZone", settings.time_zone.as_str()),
        ("extra_float_digits", "3"),
        ("application_name", settings.application_name.as_str()),
    ];
    if let Some(schema) = &settings.current_schema {
        params.push(("search_path", schema.as_str()));
    }
    if let Some(options) = &settings.options {
        params.push(("options", options.as_str()));
    }
    frontend::startup_message(params, session.output()?)?;
    session.wire_mut()?.flush().await?;
    authenticate(&mut session, settings).await?;
    Ok(session)
}

fn is_invalid_authorization(error: &StartupError) -> bool {
    matches!(error, StartupError::Sql(e) if e.sql_state() == Some(INVALID_AUTHORIZATION))
}

fn is_timeout(error: &StartupError) -> bool {
    matches!(error, StartupError::Io(e) if e.kind() == io::ErrorKind::TimedOut)
}

async fn connect_host(
    host: &HostSpec,
    settings: &ConnectSettings,
    started: Instant,
) -> Result<Session, StartupError> {
    let mode = settings.ssl_mode;
    let first = try_connect(host, mode, settings, started).await;
    let fallback = match &first {
        Err(e) if mode == SslMode::Prefer && (is_invalid_authorization(e) || is_timeout(e)) => {
            SslMode::Disable
        }
        Err(e)
            if mode == SslMode::Allow
                && (is_invalid_authorization(e) || matches!(e, StartupError::Io(_))) =>
        {
            SslMode::Require
        }
        _ => return first,
    };
    tracing::debug!(
        "sslmode {:?} retrying {}:{} with sslmode {:?}",
        mode,
        host.host,
        host.port,
        fallback
    );
    match try_connect(host, fallback, settings, started).await {
        Ok(session) => Ok(session),
        Err(retry) => {
            tracing::debug!(
                "retry without the first sslmode failed as well: {:?}",
                retry
            );
            first
        }
    }
}

fn final_error(host: &HostSpec, error: StartupError) -> SqlError {
    match error {
        StartupError::Sql(e) => e,
        StartupError::Io(e) if e.kind() == io::ErrorKind::ConnectionRefused => psql_with_cause(
            format!(
                "Connection to {}:{} refused. Check that the hostname and port are correct and that the postmaster is accepting TCP/IP connections.",
                host.host, host.port
            ),
            CONNECTION_UNABLE_TO_CONNECT,
            e,
        ),
        StartupError::Io(e) => psql_with_cause(
            "The connection attempt failed.",
            CONNECTION_UNABLE_TO_CONNECT,
            e,
        ),
    }
}

async fn connect_hosts(settings: &ConnectSettings) -> Result<(Session, HostSpec), SqlError> {
    let started = Instant::now();
    let mut last = None;
    for host in &settings.hosts {
        match connect_host(host, settings, started).await {
            Ok(session) => return Ok((session, host.clone())),
            Err(error) => {
                tracing::debug!(
                    "Connection to {}:{} failed: {:?}",
                    host.host,
                    host.port,
                    error
                );
                last = Some(final_error(host, error));
            }
        }
    }
    Err(last.unwrap_or_else(|| {
        psql(
            "The connection attempt failed.",
            CONNECTION_UNABLE_TO_CONNECT,
        )
    }))
}

pub(crate) async fn connect(settings: &ConnectSettings) -> Result<(Session, HostSpec), SqlError> {
    match settings.login_timeout {
        Some(limit) => tokio::time::timeout(limit, connect_hosts(settings))
            .await
            .unwrap_or_else(|_| {
                Err(psql(
                    "Connection attempt timed out.",
                    CONNECTION_UNABLE_TO_CONNECT,
                ))
            }),
        None => connect_hosts(settings).await,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CancelTarget {
    pub(crate) host: HostSpec,
    pub(crate) key: BackendKey,
    pub(crate) timeout: Option<Duration>,
}

pub(crate) async fn send_cancel(target: &CancelTarget) -> io::Result<()> {
    let work = async {
        let mut stream = TcpStream::connect((target.host.host.as_str(), target.host.port)).await?;
        let mut request = BytesMut::new();
        frontend::cancel_request(target.key.process_id, target.key.secret_key, &mut request);
        stream.write_all(&request).await?;
        stream.flush().await?;
        let mut rest = Vec::new();
        stream.read_to_end(&mut rest).await?;
        Ok(())
    };
    within(target.timeout, "cancel timed out", work).await
}
