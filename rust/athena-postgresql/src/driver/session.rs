use std::collections::{HashMap, VecDeque};
use std::io;
use std::sync::{Arc, Mutex as StdMutex, MutexGuard};
use std::time::Duration;

use athena_jdbc::sql::{SqlError, SqlValue};
use bytes::BytesMut;
use fallible_iterator::FallibleIterator;
use postgres_protocol::message::backend::{ErrorFields, Message, RowDescriptionBody};
use postgres_protocol::message::frontend;
use postgres_protocol::IsNull;

use super::codec::{encode_parameter, Decoder};
use super::error::{connection_closed, io_failure, protocol_violation, server_error};
use super::types::{LookedUpType, TypeCache, TYPE_LOOKUP_QUERY};
use super::wire::Wire;

const STATUS_IDLE: u8 = b'I';

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FieldDescription {
    pub(crate) name: String,
    pub(crate) table_oid: u32,
    pub(crate) column: i16,
    pub(crate) type_oid: u32,
    pub(crate) type_modifier: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BackendKey {
    pub(crate) process_id: i32,
    pub(crate) secret_key: i32,
}

#[derive(Debug, Default)]
pub(crate) struct Buffered {
    rows: VecDeque<Vec<SqlValue>>,
    done: bool,
    abandoned: bool,
    error: Option<SqlError>,
}

pub(crate) type SharedBuffer = Arc<StdMutex<Buffered>>;

pub(crate) fn lock_buffer(buffer: &SharedBuffer) -> MutexGuard<'_, Buffered> {
    buffer
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Buffered {
    pub(crate) fn pop(&mut self) -> Option<Result<Option<Vec<SqlValue>>, SqlError>> {
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

    fn finish(&mut self, error: Option<SqlError>) {
        self.done = true;
        if self.error.is_none() {
            self.error = error;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Portal {
    name: String,
    fetch_size: i32,
}

struct ResultStream {
    decoders: Arc<[Decoder]>,
    buffer: SharedBuffer,
    portal: Option<Portal>,
}

struct ActiveResult {
    id: u64,
    stream: ResultStream,
    suspended: bool,
    error: Option<SqlError>,
}

pub(crate) struct Described {
    pub(crate) parameter_types: Vec<u32>,
    pub(crate) fields: Option<Vec<FieldDescription>>,
}

pub(crate) struct Executed {
    pub(crate) fields: Option<Vec<FieldDescription>>,
    pub(crate) result: Option<(u64, SharedBuffer)>,
}

pub(crate) struct SessionOptions {
    pub(crate) string_type_unspecified: bool,
    pub(crate) log_server_error_detail: bool,
    pub(crate) socket_timeout: Option<Duration>,
}

pub(crate) struct Session {
    wire: Option<Wire>,
    tx_status: u8,
    auto_commit: bool,
    types: TypeCache,
    active: Option<ActiveResult>,
    portals: HashMap<u64, ResultStream>,
    next_id: u64,
    pending_syncs: u32,
    unsynced: bool,
    options: SessionOptions,
    backend_key: Option<BackendKey>,
}

fn error_fields(fields: ErrorFields<'_>) -> Vec<(u8, String)> {
    let mut out = Vec::new();
    let mut fields = fields;
    while let Ok(Some(field)) = fields.next() {
        out.push((
            field.type_(),
            String::from_utf8_lossy(field.value_bytes()).into_owned(),
        ));
    }
    out
}

fn field_descriptions(body: &RowDescriptionBody) -> Result<Vec<FieldDescription>, SqlError> {
    body.fields()
        .map(|f| {
            Ok(FieldDescription {
                name: f.name().to_string(),
                table_oid: f.table_oid(),
                column: f.column_id(),
                type_oid: f.type_oid(),
                type_modifier: f.type_modifier(),
            })
        })
        .collect()
        .map_err(|e| protocol_violation(format!("Invalid RowDescription message: {e}")))
}

fn encoding_error(message: &str) -> SqlError {
    io_failure(io::Error::new(
        io::ErrorKind::InvalidInput,
        message.to_string(),
    ))
}

impl Session {
    pub(crate) fn new(wire: Wire, options: SessionOptions) -> Session {
        Session {
            wire: Some(wire),
            tx_status: STATUS_IDLE,
            auto_commit: true,
            types: TypeCache::default(),
            active: None,
            portals: HashMap::new(),
            next_id: 0,
            pending_syncs: 0,
            unsynced: false,
            options,
            backend_key: None,
        }
    }

    pub(crate) fn wire_mut(&mut self) -> Result<&mut Wire, SqlError> {
        self.wire.as_mut().ok_or_else(connection_closed)
    }

    pub(crate) fn wire_ref(&self) -> Option<&Wire> {
        self.wire.as_ref()
    }

    pub(crate) fn set_backend_key(&mut self, key: BackendKey) {
        self.backend_key = Some(key);
    }

    pub(crate) fn backend_key(&self) -> Option<BackendKey> {
        self.backend_key
    }

    pub(crate) fn set_tx_status(&mut self, status: u8) {
        self.tx_status = status;
    }

    pub(crate) fn server_error(&self, fields: ErrorFields<'_>) -> SqlError {
        let fields = error_fields(fields);
        if self.options.log_server_error_detail {
            server_error(&fields)
        } else {
            let without_detail: Vec<(u8, String)> = fields
                .into_iter()
                .filter(|(code, _)| *code != b'D')
                .collect();
            server_error(&without_detail)
        }
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

    pub(crate) fn output(&mut self) -> Result<&mut BytesMut, SqlError> {
        Ok(self.wire_mut()?.output())
    }

    fn queue_sync(&mut self) -> Result<(), SqlError> {
        frontend::sync(self.output()?);
        self.pending_syncs += 1;
        self.unsynced = false;
        Ok(())
    }

    fn queue_query(&mut self, sql: &str) -> Result<(), SqlError> {
        frontend::query(sql, self.output()?).map_err(io_failure)?;
        self.pending_syncs += 1;
        Ok(())
    }

    pub(crate) async fn flush(&mut self) -> Result<(), SqlError> {
        let result = self.wire_mut()?.flush().await;
        result.map_err(|e| self.broken(e))
    }

    pub(crate) async fn receive_io(&mut self) -> Result<Message, io::Error> {
        let timeout = self.options.socket_timeout;
        let wire = self
            .wire
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "connection closed"))?;
        let message = match timeout {
            Some(limit) => tokio::time::timeout(limit, wire.receive())
                .await
                .unwrap_or_else(|_| Err(io::Error::new(io::ErrorKind::TimedOut, "Read timed out"))),
            None => wire.receive().await,
        };
        match message {
            Ok(Message::ReadyForQuery(body)) => {
                self.pending_syncs = self.pending_syncs.saturating_sub(1);
                self.tx_status = body.status();
                Ok(Message::ReadyForQuery(body))
            }
            Ok(other) => Ok(other),
            Err(e) => {
                self.wire = None;
                self.abort_results();
                Err(e)
            }
        }
    }

    async fn receive(&mut self) -> Result<Message, SqlError> {
        self.receive_io().await.map_err(io_failure)
    }

    fn abort_results(&mut self) {
        if let Some(active) = self.active.take() {
            lock_buffer(&active.stream.buffer).finish(Some(connection_closed()));
        }
        for (_, stream) in self.portals.drain() {
            lock_buffer(&stream.buffer).finish(Some(connection_closed()));
        }
    }

    fn broken(&mut self, cause: io::Error) -> SqlError {
        self.wire = None;
        self.abort_results();
        io_failure(cause)
    }

    async fn read_until_idle(&mut self) -> Result<Option<SqlError>, SqlError> {
        let mut error = None;
        while self.pending_syncs > 0 {
            if let Message::ErrorResponse(body) = self.receive().await? {
                if error.is_none() {
                    error = Some(self.server_error(body.fields()));
                }
            }
        }
        Ok(error)
    }

    async fn resync(&mut self) -> Result<(), SqlError> {
        if self.unsynced {
            self.queue_sync()?;
        }
        if self.pending_syncs > 0 {
            self.flush().await?;
            self.read_until_idle().await?;
        }
        Ok(())
    }

    pub(crate) fn active_id(&self) -> Option<u64> {
        self.active.as_ref().map(|a| a.id)
    }

    pub(crate) fn has_portal(&self, id: u64) -> bool {
        self.portals.contains_key(&id)
    }

    pub(crate) async fn finish_active(&mut self) -> Result<(), SqlError> {
        while let Some(active) = &self.active {
            let (id, buffer) = (active.id, Arc::clone(&active.stream.buffer));
            match self.next_row(id, false).await {
                Ok(Some(row)) => {
                    let mut buffered = lock_buffer(&buffer);
                    if !buffered.abandoned {
                        buffered.rows.push_back(row);
                    }
                }
                Ok(None) => {}
                Err(e) => lock_buffer(&buffer).finish(Some(e)),
            }
        }
        self.portals
            .retain(|_, stream| !lock_buffer(&stream.buffer).abandoned);
        Ok(())
    }

    fn decode_row(
        decoders: &[Decoder],
        body: &postgres_protocol::message::backend::DataRowBody,
    ) -> Result<Vec<SqlValue>, SqlError> {
        let buffer = body.buffer();
        body.ranges()
            .enumerate()
            .map(|(i, range)| {
                let raw = range.map(|r| &buffer[r]);
                Ok(decoders.get(i).unwrap_or(&Decoder::Text).decode(raw))
            })
            .collect()
            .map_err(|e| protocol_violation(format!("Invalid DataRow message: {e}")))
    }

    pub(crate) async fn next_row(
        &mut self,
        id: u64,
        fetch_more: bool,
    ) -> Result<Option<Vec<SqlValue>>, SqlError> {
        loop {
            let Some(decoders) = self
                .active
                .as_ref()
                .filter(|a| a.id == id)
                .map(|a| Arc::clone(&a.stream.decoders))
            else {
                return Ok(None);
            };
            match self.receive().await? {
                Message::DataRow(body) => {
                    return Self::decode_row(&decoders, &body).map(Some);
                }
                Message::PortalSuspended => {
                    if let Some(active) = self.active.as_mut() {
                        active.suspended = true;
                    }
                }
                Message::ErrorResponse(body) => {
                    let error = self.server_error(body.fields());
                    if let Some(active) = self.active.as_mut() {
                        active.error.get_or_insert(error);
                    }
                }
                Message::ReadyForQuery(_) if self.pending_syncs == 0 => {
                    let Some(active) = self.active.take() else {
                        return Ok(None);
                    };
                    if let Some(error) = active.error {
                        lock_buffer(&active.stream.buffer).finish(Some(error.clone()));
                        return Err(error);
                    }
                    if !active.suspended {
                        lock_buffer(&active.stream.buffer).finish(None);
                        return Ok(None);
                    }
                    if !fetch_more {
                        self.portals.insert(id, active.stream);
                        return Ok(None);
                    }
                    self.fetch(id, active.stream)?;
                    self.flush().await?;
                }
                _ => {}
            }
        }
    }

    fn fetch(&mut self, id: u64, stream: ResultStream) -> Result<(), SqlError> {
        let portal = stream
            .portal
            .clone()
            .ok_or_else(|| protocol_violation("Portal suspended without a fetch size"))?;
        frontend::execute(&portal.name, portal.fetch_size, self.output()?).map_err(io_failure)?;
        self.queue_sync()?;
        self.active = Some(ActiveResult {
            id,
            stream,
            suspended: false,
            error: None,
        });
        Ok(())
    }

    pub(crate) async fn resume(&mut self, id: u64) -> Result<(), SqlError> {
        self.prepare().await?;
        if let Some(stream) = self.portals.remove(&id) {
            self.fetch(id, stream)?;
            self.flush().await?;
        }
        Ok(())
    }

    pub(crate) async fn prepare(&mut self) -> Result<(), SqlError> {
        if self.wire.is_none() {
            return Err(connection_closed());
        }
        self.finish_active().await?;
        self.resync().await
    }

    pub(crate) async fn simple_query(&mut self, sql: &str) -> Result<(), SqlError> {
        self.prepare().await?;
        self.queue_query(sql)?;
        self.flush().await?;
        match self.read_until_idle().await? {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub(crate) async fn set_auto_commit(&mut self, auto_commit: bool) -> Result<(), SqlError> {
        if auto_commit && !self.auto_commit && self.tx_status != STATUS_IDLE {
            self.simple_query("COMMIT").await?;
        }
        self.auto_commit = auto_commit;
        Ok(())
    }

    pub(crate) async fn commit(&mut self) -> Result<(), SqlError> {
        if self.auto_commit {
            return Err(super::error::psql(
                "Cannot commit when autoCommit is enabled.",
                super::error::INVALID_TRANSACTION_STATE,
            ));
        }
        self.prepare().await?;
        if self.tx_status != STATUS_IDLE {
            self.simple_query("COMMIT").await?;
        }
        Ok(())
    }

    pub(crate) async fn rollback(&mut self) -> Result<(), SqlError> {
        if self.auto_commit {
            return Err(super::error::psql(
                "Cannot rollback when autoCommit is enabled.",
                super::error::INVALID_TRANSACTION_STATE,
            ));
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
            frontend::terminate(wire.output());
            if let Err(e) = wire.flush().await {
                tracing::debug!("Failed to send terminate message: {}", e);
            }
            if let Err(e) = wire.shutdown().await {
                tracing::debug!("Failed to shut down the socket: {}", e);
            }
        }
        self.abort_results();
    }

    pub(crate) async fn describe(&mut self, sql: &str) -> Result<Described, SqlError> {
        self.prepare().await?;
        let out = self.output()?;
        frontend::parse("", sql, std::iter::empty(), out).map_err(io_failure)?;
        frontend::describe(b'S', "", out).map_err(io_failure)?;
        self.queue_sync()?;
        self.flush().await?;
        let mut parameter_types = Vec::new();
        let mut fields = None;
        let mut error = None;
        while self.pending_syncs > 0 {
            match self.receive().await? {
                Message::ParameterDescription(body) => {
                    parameter_types = body.parameters().collect().map_err(|e| {
                        protocol_violation(format!("Invalid ParameterDescription message: {e}"))
                    })?;
                }
                Message::RowDescription(body) => fields = Some(field_descriptions(&body)?),
                Message::ErrorResponse(body) if error.is_none() => {
                    error = Some(self.server_error(body.fields()));
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

    fn queue_parse_bind(
        &mut self,
        sql: &str,
        portal: &str,
        oids: &[u32],
        values: &[Option<String>],
    ) -> Result<(), SqlError> {
        let out = self.output()?;
        frontend::parse("", sql, oids.iter().copied(), out).map_err(io_failure)?;
        frontend::bind(
            portal,
            "",
            std::iter::empty(),
            values,
            |value, buf| match value {
                Some(text) => {
                    buf.extend_from_slice(text.as_bytes());
                    Ok(IsNull::No)
                }
                None => Ok(IsNull::Yes),
            },
            std::iter::empty(),
            out,
        )
        .map_err(|_| encoding_error("Unable to encode the bind parameters"))?;
        frontend::describe(b'P', portal, out).map_err(io_failure)?;
        frontend::flush(out);
        self.unsynced = true;
        Ok(())
    }

    pub(crate) async fn execute(
        &mut self,
        sql: &str,
        parameters: &[SqlValue],
        suppress_begin: bool,
        fetch_size: i32,
    ) -> Result<Executed, SqlError> {
        let (oids, values): (Vec<u32>, Vec<Option<String>>) = parameters
            .iter()
            .map(|p| encode_parameter(p, self.options.string_type_unspecified))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .unzip();
        self.prepare().await?;
        let use_portal = fetch_size > 0 && !self.auto_commit;
        let mut retried = false;
        loop {
            let begin = !suppress_begin && !self.auto_commit && self.tx_status == STATUS_IDLE;
            if begin {
                self.queue_query("BEGIN")?;
            }
            self.next_id += 1;
            let id = self.next_id;
            let portal = use_portal.then(|| Portal {
                name: format!("C_{id}"),
                fetch_size,
            });
            let portal_name = portal.as_ref().map_or("", |p| p.name.as_str()).to_string();
            self.queue_parse_bind(sql, &portal_name, &oids, &values)?;
            self.flush().await?;
            if begin {
                if let Some(error) = self.read_until_idle().await? {
                    self.resync().await?;
                    return Err(error);
                }
            }
            let mut fields = None;
            loop {
                match self.receive().await? {
                    Message::RowDescription(body) => {
                        fields = Some(field_descriptions(&body)?);
                        break;
                    }
                    Message::NoData => break,
                    Message::ErrorResponse(body) => {
                        let error = self.server_error(body.fields());
                        self.resync().await?;
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
                self.resync().await?;
                self.resolve_types(unknown).await?;
                retried = true;
                continue;
            }
            let max_rows = portal.as_ref().map_or(0, |p| p.fetch_size);
            frontend::execute(&portal_name, max_rows, self.output()?).map_err(io_failure)?;
            self.queue_sync()?;
            self.flush().await?;
            let Some(fields) = fields else {
                if let Some(error) = self.read_until_idle().await? {
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
            let buffer: SharedBuffer = Arc::new(StdMutex::new(Buffered::default()));
            self.active = Some(ActiveResult {
                id,
                stream: ResultStream {
                    decoders,
                    buffer: Arc::clone(&buffer),
                    portal,
                },
                suspended: false,
                error: None,
            });
            return Ok(Executed {
                fields: Some(fields),
                result: Some((id, buffer)),
            });
        }
    }

    pub(crate) async fn query_all(
        &mut self,
        sql: &str,
        parameters: &[SqlValue],
        suppress_begin: bool,
    ) -> Result<(Vec<FieldDescription>, Vec<Vec<SqlValue>>), SqlError> {
        let executed = self.execute(sql, parameters, suppress_begin, 0).await?;
        let fields = executed.fields.unwrap_or_default();
        let mut rows = Vec::new();
        if let Some((id, _)) = executed.result {
            while let Some(row) = self.next_row(id, true).await? {
                rows.push(row);
            }
        }
        Ok((fields, rows))
    }

    pub(crate) async fn resolve_types(&mut self, oids: Vec<u32>) -> Result<(), SqlError> {
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
