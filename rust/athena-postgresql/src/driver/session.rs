use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Mutex as StdMutex, MutexGuard};
use std::time::Duration;

use athena_jdbc::sql::{SqlException, SqlValue};
use rustls_pki_types::ServerName;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

use super::auth::{md5_password, ScramClient, SCRAM_SHA_256};
use super::codec::{encode_parameter, Decoder};
use super::error::{
    connection_closed, io_failure, protocol_violation, psql, psql_with_cause, server_error,
    CONNECTION_REJECTED, CONNECTION_UNABLE_TO_CONNECT, INVALID_PASSWORD, NOT_IMPLEMENTED,
};
use super::messages::{Backend, FieldDescription, Frontend, Wire};
use super::stream::PgSocket;
use super::tls::client_config;
use super::types::{LookedUpType, TypeCache, TYPE_LOOKUP_QUERY};
use super::url::{ConnectSettings, HostSpec, SslMode};

const STATUS_IDLE: u8 = b'I';

#[derive(Debug, Default)]
pub(crate) struct Buffered {
    rows: VecDeque<Vec<SqlValue>>,
    done: bool,
    abandoned: bool,
    error: Option<SqlException>,
}

pub(crate) type SharedBuffer = Arc<StdMutex<Buffered>>;

pub(crate) fn lock_buffer(buffer: &SharedBuffer) -> MutexGuard<'_, Buffered> {
    buffer
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Buffered {
    pub(crate) fn pop(&mut self) -> Option<Result<Option<Vec<SqlValue>>, SqlException>> {
        if let Some(row) = self.rows.pop_front() {
            return Some(Ok(Some(row)));
        }
        if self.done {
            return Some(self.error.take().map_or(Ok(None), Err));
        }
        None
    }

    pub(crate) fn abandon(&mut self) {
        self.abandoned = true;
        self.rows.clear();
    }
}

struct ActiveResult {
    id: u64,
    decoders: Arc<[Decoder]>,
    buffer: SharedBuffer,
}

pub(crate) struct Described {
    pub(crate) parameter_types: Vec<u32>,
    pub(crate) fields: Option<Vec<FieldDescription>>,
}

pub(crate) struct Executed {
    pub(crate) fields: Option<Vec<FieldDescription>>,
    pub(crate) result: Option<(u64, SharedBuffer)>,
}

pub(crate) struct Session {
    wire: Option<Wire>,
    tx_status: u8,
    auto_commit: bool,
    types: TypeCache,
    active: Option<ActiveResult>,
    next_result_id: u64,
    string_type_unspecified: bool,
    log_server_error_detail: bool,
    socket_timeout: Option<Duration>,
    user: Option<String>,
}

fn connect_error(host: &HostSpec, cause: io::Error) -> SqlException {
    if cause.kind() == io::ErrorKind::ConnectionRefused {
        psql_with_cause(
            format!(
                "Connection to {}:{} refused. Check that the hostname and port are correct and that the postmaster is accepting TCP/IP connections.",
                host.host, host.port
            ),
            CONNECTION_UNABLE_TO_CONNECT,
            cause,
        )
    } else {
        psql_with_cause(
            "The connection attempt failed.",
            CONNECTION_UNABLE_TO_CONNECT,
            cause,
        )
    }
}

async fn open_tcp(host: &HostSpec, settings: &ConnectSettings) -> Result<TcpStream, SqlException> {
    let connect = TcpStream::connect((host.host.as_str(), host.port));
    let stream = match settings.connect_timeout {
        Some(limit) => tokio::time::timeout(limit, connect).await.map_err(|_| {
            connect_error(
                host,
                io::Error::new(io::ErrorKind::TimedOut, "connect timed out"),
            )
        })?,
        None => connect.await,
    }
    .map_err(|e| connect_error(host, e))?;
    stream
        .set_nodelay(true)
        .map_err(|e| connect_error(host, e))?;
    Ok(stream)
}

async fn negotiate_tls(
    mut stream: TcpStream,
    host: &HostSpec,
    settings: &ConnectSettings,
) -> Result<Option<PgSocket>, SqlException> {
    let mut frontend = Frontend::default();
    frontend.ssl_request();
    stream
        .write_all(&frontend.take())
        .await
        .map_err(|e| connect_error(host, e))?;
    let answer = stream.read_u8().await.map_err(|e| connect_error(host, e))?;
    match answer {
        b'S' => {
            let config = client_config(
                &settings.ssl_verification,
                settings.ssl_root_cert.as_deref(),
            )?;
            let server_name = ServerName::try_from(host.host.clone()).map_err(|e| {
                psql(
                    format!("The hostname {} could not be verified. {e}", host.host),
                    CONNECTION_UNABLE_TO_CONNECT,
                )
            })?;
            let tls = TlsConnector::from(config)
                .connect(server_name, stream)
                .await
                .map_err(|e| {
                    psql_with_cause(
                        "SSL error: ".to_string() + &e.to_string(),
                        CONNECTION_UNABLE_TO_CONNECT,
                        e,
                    )
                })?;
            Ok(Some(PgSocket::Tls(Box::new(tls))))
        }
        b'N' => {
            if settings.ssl_mode.requires_ssl() {
                return Err(psql(
                    "The server does not support SSL.",
                    CONNECTION_REJECTED,
                ));
            }
            Ok(Some(PgSocket::Plain(stream)))
        }
        b'E' => Ok(None),
        other => Err(protocol_violation(format!(
            "An error occurred while setting up the SSL connection: unexpected response {other}"
        ))),
    }
}

async fn open_socket(
    host: &HostSpec,
    settings: &ConnectSettings,
) -> Result<PgSocket, SqlException> {
    match settings.ssl_mode {
        SslMode::Disable | SslMode::Allow => Ok(PgSocket::Plain(open_tcp(host, settings).await?)),
        _ => {
            let stream = open_tcp(host, settings).await?;
            match negotiate_tls(stream, host, settings).await {
                Ok(Some(socket)) => Ok(socket),
                Ok(None) => Ok(PgSocket::Plain(open_tcp(host, settings).await?)),
                Err(e)
                    if settings.ssl_mode == SslMode::Prefer
                        && e.sql_state() == Some(CONNECTION_UNABLE_TO_CONNECT) =>
                {
                    tracing::debug!("SSL connection failed, retrying without SSL: {}", e);
                    Ok(PgSocket::Plain(open_tcp(host, settings).await?))
                }
                Err(e) => Err(e),
            }
        }
    }
}

fn no_password() -> SqlException {
    psql(
        "The server requested password-based authentication, but no password was provided by plugin null",
        INVALID_PASSWORD,
    )
}

impl Session {
    pub(crate) async fn connect(settings: &ConnectSettings) -> Result<Session, SqlException> {
        let mut last_error = None;
        for host in &settings.hosts {
            match Session::connect_host(host, settings).await {
                Ok(session) => return Ok(session),
                Err(e) => {
                    tracing::debug!("Connection to {}:{} failed: {}", host.host, host.port, e);
                    last_error = Some(e);
                }
            }
        }
        Err(last_error.unwrap_or_else(|| {
            psql(
                "The connection attempt failed.",
                CONNECTION_UNABLE_TO_CONNECT,
            )
        }))
    }

    async fn connect_host(
        host: &HostSpec,
        settings: &ConnectSettings,
    ) -> Result<Session, SqlException> {
        let socket = open_socket(host, settings).await?;
        let mut session = Session {
            wire: Some(Wire::new(socket)),
            tx_status: STATUS_IDLE,
            auto_commit: true,
            types: TypeCache::default(),
            active: None,
            next_result_id: 0,
            string_type_unspecified: settings.string_type_unspecified,
            log_server_error_detail: settings.log_server_error_detail,
            socket_timeout: settings.socket_timeout,
            user: settings.user.clone(),
        };
        let user = settings.user.clone().unwrap_or_default();
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
        let mut frontend = Frontend::default();
        frontend.startup(&params).map_err(io_failure)?;
        session.send(&mut frontend).await?;
        session
            .authenticate(&user, settings.password.as_deref())
            .await?;
        Ok(session)
    }

    async fn authenticate(
        &mut self,
        user: &str,
        password: Option<&str>,
    ) -> Result<(), SqlException> {
        let mut scram: Option<ScramClient> = None;
        loop {
            match self.receive().await? {
                Backend::Authentication { code: 0, .. } => {}
                Backend::Authentication { code: 3, .. } => {
                    let password = password.ok_or_else(no_password)?;
                    let mut frontend = Frontend::default();
                    frontend.password(password.as_bytes()).map_err(io_failure)?;
                    self.send(&mut frontend).await?;
                }
                Backend::Authentication { code: 5, data } => {
                    let password = password.ok_or_else(no_password)?;
                    let hashed = md5_password(user, password, &data);
                    let mut frontend = Frontend::default();
                    frontend.password(hashed.as_bytes()).map_err(io_failure)?;
                    self.send(&mut frontend).await?;
                }
                Backend::Authentication { code: 10, data } => {
                    let password = password.ok_or_else(no_password)?;
                    let mechanisms: Vec<String> = data
                        .split(|b| *b == 0)
                        .filter(|m| !m.is_empty())
                        .map(|m| String::from_utf8_lossy(m).into_owned())
                        .collect();
                    if !mechanisms.iter().any(|m| m == SCRAM_SHA_256) {
                        return Err(psql(
                            format!(
                                "SCRAM authentication failed: none of the server's mechanisms ({}) are supported",
                                mechanisms.join(", ")
                            ),
                            NOT_IMPLEMENTED,
                        ));
                    }
                    let client = ScramClient::new(password)?;
                    let mut frontend = Frontend::default();
                    frontend
                        .sasl_initial_response(SCRAM_SHA_256, client.client_first().as_bytes())
                        .map_err(io_failure)?;
                    self.send(&mut frontend).await?;
                    scram = Some(client);
                }
                Backend::Authentication { code: 11, data } => {
                    let client = scram
                        .as_mut()
                        .ok_or_else(|| protocol_violation("Unexpected SASL continue message"))?;
                    let response = client.client_final(&data)?;
                    let mut frontend = Frontend::default();
                    frontend
                        .sasl_response(response.as_bytes())
                        .map_err(io_failure)?;
                    self.send(&mut frontend).await?;
                }
                Backend::Authentication { code: 12, data } => {
                    scram
                        .as_ref()
                        .ok_or_else(|| protocol_violation("Unexpected SASL final message"))?
                        .verify_server_final(&data)?;
                }
                Backend::Authentication { code, .. } => {
                    return Err(psql(
                        format!(
                            "The authentication type {code} is not supported. Check that you have configured the pg_hba.conf file to include the client's IP address or subnet, and that it is using an authentication scheme supported by the driver."
                        ),
                        CONNECTION_REJECTED,
                    ));
                }
                Backend::Error(fields) => return Err(self.server_error(&fields)),
                Backend::ReadyForQuery(status) => {
                    self.tx_status = status;
                    return Ok(());
                }
                _ => {}
            }
        }
    }

    fn server_error(&self, fields: &[(u8, String)]) -> SqlException {
        if self.log_server_error_detail {
            server_error(fields)
        } else {
            let without_detail: Vec<(u8, String)> = fields
                .iter()
                .filter(|(code, _)| *code != b'D')
                .cloned()
                .collect();
            server_error(&without_detail)
        }
    }

    pub(crate) fn user(&self) -> Option<&str> {
        self.user.as_deref()
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.wire.is_none()
    }

    pub(crate) fn auto_commit(&self) -> bool {
        self.auto_commit
    }

    pub(crate) fn types(&self) -> &TypeCache {
        &self.types
    }

    fn wire(&mut self) -> Result<&mut Wire, SqlException> {
        self.wire.as_mut().ok_or_else(connection_closed)
    }

    async fn send(&mut self, frontend: &mut Frontend) -> Result<(), SqlException> {
        let bytes = frontend.take();
        let result = self.wire()?.send(&bytes).await;
        result.map_err(|e| self.broken(e))
    }

    async fn receive(&mut self) -> Result<Backend, SqlException> {
        let timeout = self.socket_timeout;
        let wire = self.wire()?;
        let result = match timeout {
            Some(limit) => match tokio::time::timeout(limit, wire.receive()).await {
                Ok(r) => r,
                Err(_) => Err(io::Error::new(io::ErrorKind::TimedOut, "Read timed out")),
            },
            None => wire.receive().await,
        };
        result.map_err(|e| self.broken(e))
    }

    fn broken(&mut self, cause: io::Error) -> SqlException {
        self.wire = None;
        if let Some(active) = self.active.take() {
            let mut buffer = lock_buffer(&active.buffer);
            buffer.done = true;
            buffer.error = Some(connection_closed());
        }
        io_failure(cause)
    }

    async fn read_until_ready(&mut self) -> Result<Option<SqlException>, SqlException> {
        let mut error = None;
        loop {
            match self.receive().await? {
                Backend::ReadyForQuery(status) => {
                    self.tx_status = status;
                    return Ok(error);
                }
                Backend::Error(fields) if error.is_none() => {
                    error = Some(self.server_error(&fields));
                }
                _ => {}
            }
        }
    }

    pub(crate) fn active_id(&self) -> Option<u64> {
        self.active.as_ref().map(|a| a.id)
    }

    pub(crate) async fn finish_active(&mut self) -> Result<(), SqlException> {
        while let Some(active) = &self.active {
            let (id, buffer) = (active.id, Arc::clone(&active.buffer));
            match self.next_row(id).await {
                Ok(Some(row)) => {
                    let mut buffered = lock_buffer(&buffer);
                    if !buffered.abandoned {
                        buffered.rows.push_back(row);
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    let mut buffered = lock_buffer(&buffer);
                    buffered.done = true;
                    buffered.error = Some(e);
                }
            }
        }
        Ok(())
    }

    pub(crate) async fn next_row(
        &mut self,
        id: u64,
    ) -> Result<Option<Vec<SqlValue>>, SqlException> {
        let Some(decoders) = self
            .active
            .as_ref()
            .filter(|a| a.id == id)
            .map(|a| Arc::clone(&a.decoders))
        else {
            return Ok(None);
        };
        loop {
            match self.receive().await? {
                Backend::DataRow(values) => {
                    let row = values
                        .into_iter()
                        .enumerate()
                        .map(|(i, raw)| match decoders.get(i) {
                            Some(decoder) => decoder.decode(raw),
                            None => Decoder::Text.decode(raw),
                        })
                        .collect();
                    return Ok(Some(row));
                }
                Backend::Error(fields) => {
                    let error = self.server_error(&fields);
                    self.read_until_ready().await?;
                    self.finish_result(None);
                    return Err(error);
                }
                Backend::ReadyForQuery(status) => {
                    self.tx_status = status;
                    self.finish_result(None);
                    return Ok(None);
                }
                _ => {}
            }
        }
    }

    fn finish_result(&mut self, error: Option<SqlException>) {
        if let Some(active) = self.active.take() {
            let mut buffer = lock_buffer(&active.buffer);
            buffer.done = true;
            if buffer.error.is_none() {
                buffer.error = error;
            }
        }
    }

    pub(crate) async fn prepare(&mut self) -> Result<(), SqlException> {
        if self.wire.is_none() {
            return Err(connection_closed());
        }
        self.finish_active().await
    }

    pub(crate) async fn simple_query(&mut self, sql: &str) -> Result<(), SqlException> {
        self.prepare().await?;
        let mut frontend = Frontend::default();
        frontend.query(sql).map_err(io_failure)?;
        self.send(&mut frontend).await?;
        match self.read_until_ready().await? {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub(crate) async fn set_auto_commit(&mut self, auto_commit: bool) -> Result<(), SqlException> {
        if auto_commit && !self.auto_commit && self.tx_status != STATUS_IDLE {
            self.simple_query("COMMIT").await?;
        }
        self.auto_commit = auto_commit;
        Ok(())
    }

    pub(crate) async fn commit(&mut self) -> Result<(), SqlException> {
        if self.auto_commit {
            return Err(psql("Cannot commit when autoCommit is enabled.", "25000"));
        }
        self.prepare().await?;
        if self.tx_status != STATUS_IDLE {
            self.simple_query("COMMIT").await?;
        }
        Ok(())
    }

    pub(crate) async fn rollback(&mut self) -> Result<(), SqlException> {
        if self.auto_commit {
            return Err(psql("Cannot rollback when autoCommit is enabled.", "25000"));
        }
        self.prepare().await?;
        if self.tx_status != STATUS_IDLE {
            self.simple_query("ROLLBACK").await?;
        }
        Ok(())
    }

    pub(crate) async fn is_valid(&mut self) -> bool {
        self.simple_query("").await.is_ok()
    }

    pub(crate) async fn close(&mut self) {
        if let Some(mut wire) = self.wire.take() {
            let mut frontend = Frontend::default();
            if frontend.terminate().is_ok() {
                if let Err(e) = wire.send(&frontend.take()).await {
                    tracing::debug!("Failed to send terminate message: {}", e);
                }
            }
            if let Err(e) = wire.shutdown().await {
                tracing::debug!("Failed to shut down the socket: {}", e);
            }
        }
        self.finish_result(Some(connection_closed()));
    }

    pub(crate) async fn describe(&mut self, sql: &str) -> Result<Described, SqlException> {
        self.prepare().await?;
        let mut frontend = Frontend::default();
        frontend.parse("", sql, &[]).map_err(io_failure)?;
        frontend.describe_statement("").map_err(io_failure)?;
        frontend.sync().map_err(io_failure)?;
        self.send(&mut frontend).await?;
        let mut parameter_types = Vec::new();
        let mut fields = None;
        let mut error = None;
        loop {
            match self.receive().await? {
                Backend::ParameterDescription(oids) => parameter_types = oids,
                Backend::RowDescription(f) => fields = Some(f),
                Backend::Error(e) => {
                    if error.is_none() {
                        error = Some(self.server_error(&e));
                    }
                }
                Backend::ReadyForQuery(status) => {
                    self.tx_status = status;
                    break;
                }
                _ => {}
            }
        }
        if let Some(error) = error {
            return Err(error);
        }
        if let Some(fields) = &fields {
            self.resolve_types(fields.iter().map(|f| f.type_oid).collect())
                .await?;
        }
        Ok(Described {
            parameter_types,
            fields,
        })
    }

    pub(crate) async fn execute(
        &mut self,
        sql: &str,
        parameters: &[SqlValue],
        suppress_begin: bool,
    ) -> Result<Executed, SqlException> {
        self.prepare().await?;
        let (oids, values): (Vec<u32>, Vec<Option<String>>) = parameters
            .iter()
            .map(|p| encode_parameter(p, self.string_type_unspecified))
            .unzip();
        let mut retried = false;
        loop {
            let begin = !suppress_begin && !self.auto_commit && self.tx_status == STATUS_IDLE;
            let mut frontend = Frontend::default();
            if begin {
                frontend.query("BEGIN").map_err(io_failure)?;
            }
            frontend.parse("", sql, &oids).map_err(io_failure)?;
            frontend.bind("", "", &values).map_err(io_failure)?;
            frontend.describe_portal("").map_err(io_failure)?;
            frontend.flush().map_err(io_failure)?;
            self.send(&mut frontend).await?;
            if begin {
                if let Some(error) = self.read_until_ready().await? {
                    self.sync_after_error().await?;
                    return Err(error);
                }
            }
            let mut fields = None;
            loop {
                match self.receive().await? {
                    Backend::RowDescription(f) => {
                        fields = Some(f);
                        break;
                    }
                    Backend::NoData => break,
                    Backend::Error(e) => {
                        let error = self.server_error(&e);
                        self.sync_after_error().await?;
                        return Err(error);
                    }
                    _ => {}
                }
            }
            let unknown = fields.as_ref().map_or_else(Vec::new, |f| {
                self.types
                    .missing(&f.iter().map(|d| d.type_oid).collect::<Vec<_>>())
            });
            if !unknown.is_empty() && !retried {
                self.sync_after_error().await?;
                self.resolve_types(unknown).await?;
                retried = true;
                continue;
            }
            let mut frontend = Frontend::default();
            frontend.execute("", 0).map_err(io_failure)?;
            frontend.sync().map_err(io_failure)?;
            self.send(&mut frontend).await?;
            let Some(fields) = fields else {
                if let Some(error) = self.read_until_ready().await? {
                    return Err(error);
                }
                return Ok(Executed {
                    fields: None,
                    result: None,
                });
            };
            let decoders: Arc<[Decoder]> = fields
                .iter()
                .map(|f| Decoder::for_type(&self.types, f.type_oid))
                .collect();
            self.next_result_id += 1;
            let buffer: SharedBuffer = Arc::new(StdMutex::new(Buffered::default()));
            self.active = Some(ActiveResult {
                id: self.next_result_id,
                decoders,
                buffer: Arc::clone(&buffer),
            });
            return Ok(Executed {
                fields: Some(fields),
                result: Some((self.next_result_id, buffer)),
            });
        }
    }

    async fn sync_after_error(&mut self) -> Result<(), SqlException> {
        let mut frontend = Frontend::default();
        frontend.sync().map_err(io_failure)?;
        self.send(&mut frontend).await?;
        self.read_until_ready().await?;
        Ok(())
    }

    pub(crate) async fn query_all(
        &mut self,
        sql: &str,
        parameters: &[SqlValue],
        suppress_begin: bool,
    ) -> Result<(Vec<FieldDescription>, Vec<Vec<SqlValue>>), SqlException> {
        let executed = self.execute(sql, parameters, suppress_begin).await?;
        let fields = executed.fields.unwrap_or_default();
        let mut rows = Vec::new();
        if let Some((id, _)) = executed.result {
            while let Some(row) = self.next_row(id).await? {
                rows.push(row);
            }
        }
        Ok((fields, rows))
    }

    pub(crate) async fn resolve_types(&mut self, oids: Vec<u32>) -> Result<(), SqlException> {
        let mut pending = self.types.missing(&oids);
        let mut rounds = 0;
        while !pending.is_empty() && rounds < 4 {
            rounds += 1;
            let list = format!(
                "{{{}}}",
                pending
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            );
            let (_, rows) =
                Box::pin(self.query_all(TYPE_LOOKUP_QUERY, &[SqlValue::String(list)], true))
                    .await?;
            for row in rows {
                let text = |i: usize| {
                    row.get(i)
                        .and_then(SqlValue::to_java_string)
                        .unwrap_or_default()
                };
                let number = |i: usize| text(i).parse::<u32>().unwrap_or(0);
                self.types.insert(LookedUpType {
                    oid: number(0),
                    name: text(1),
                    type_type: text(2),
                    is_array: text(3) == "t",
                    element: number(4),
                    delimiter: text(5).bytes().next().unwrap_or(b','),
                    on_path: text(6) == "t",
                    schema: text(7),
                });
            }
            for type_oid in &pending {
                self.types.insert_unknown(*type_oid);
            }
            pending = self.types.missing_elements();
        }
        Ok(())
    }
}
