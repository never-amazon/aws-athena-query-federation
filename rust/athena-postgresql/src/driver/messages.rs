use std::io;

use tokio::io::{AsyncReadExt, AsyncWriteExt, BufStream};

use super::stream::PgSocket;

pub(crate) const PROTOCOL_VERSION: i32 = 196_608;
pub(crate) const SSL_REQUEST_CODE: i32 = 80_877_103;

#[derive(Debug, Default)]
pub(crate) struct Frontend {
    buf: Vec<u8>,
}

impl Frontend {
    fn begin(&mut self, tag: u8) -> usize {
        self.buf.push(tag);
        let at = self.buf.len();
        self.buf.extend_from_slice(&[0; 4]);
        at
    }

    fn finish(&mut self, at: usize) -> io::Result<()> {
        let len = i32::try_from(self.buf.len() - at)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "message too large"))?;
        self.buf[at..at + 4].copy_from_slice(&len.to_be_bytes());
        Ok(())
    }

    fn cstr(&mut self, value: &str) {
        self.buf.extend_from_slice(value.as_bytes());
        self.buf.push(0);
    }

    fn int16(&mut self, value: i16) {
        self.buf.extend_from_slice(&value.to_be_bytes());
    }

    fn int32(&mut self, value: i32) {
        self.buf.extend_from_slice(&value.to_be_bytes());
    }

    fn count(len: usize) -> io::Result<i16> {
        i16::try_from(len)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "too many values"))
    }

    pub(crate) fn startup(&mut self, params: &[(&str, &str)]) -> io::Result<()> {
        let at = self.buf.len();
        self.buf.extend_from_slice(&[0; 4]);
        self.int32(PROTOCOL_VERSION);
        for (name, value) in params {
            self.cstr(name);
            self.cstr(value);
        }
        self.buf.push(0);
        self.finish(at)
    }

    pub(crate) fn ssl_request(&mut self) {
        self.int32(8);
        self.int32(SSL_REQUEST_CODE);
    }

    pub(crate) fn password(&mut self, password: &[u8]) -> io::Result<()> {
        let at = self.begin(b'p');
        self.buf.extend_from_slice(password);
        self.buf.push(0);
        self.finish(at)
    }

    pub(crate) fn sasl_initial_response(&mut self, mechanism: &str, data: &[u8]) -> io::Result<()> {
        let at = self.begin(b'p');
        self.cstr(mechanism);
        let len = i32::try_from(data.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "SASL data too large"))?;
        self.int32(len);
        self.buf.extend_from_slice(data);
        self.finish(at)
    }

    pub(crate) fn sasl_response(&mut self, data: &[u8]) -> io::Result<()> {
        let at = self.begin(b'p');
        self.buf.extend_from_slice(data);
        self.finish(at)
    }

    pub(crate) fn query(&mut self, sql: &str) -> io::Result<()> {
        let at = self.begin(b'Q');
        self.cstr(sql);
        self.finish(at)
    }

    pub(crate) fn parse(&mut self, name: &str, sql: &str, param_types: &[u32]) -> io::Result<()> {
        let at = self.begin(b'P');
        self.cstr(name);
        self.cstr(sql);
        self.int16(Frontend::count(param_types.len())?);
        for oid in param_types {
            self.buf.extend_from_slice(&oid.to_be_bytes());
        }
        self.finish(at)
    }

    pub(crate) fn bind(
        &mut self,
        portal: &str,
        statement: &str,
        params: &[Option<String>],
    ) -> io::Result<()> {
        let at = self.begin(b'B');
        self.cstr(portal);
        self.cstr(statement);
        self.int16(0);
        self.int16(Frontend::count(params.len())?);
        for param in params {
            match param {
                None => self.int32(-1),
                Some(text) => {
                    let len = i32::try_from(text.len()).map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidInput, "parameter too large")
                    })?;
                    self.int32(len);
                    self.buf.extend_from_slice(text.as_bytes());
                }
            }
        }
        self.int16(0);
        self.finish(at)
    }

    pub(crate) fn describe_statement(&mut self, name: &str) -> io::Result<()> {
        let at = self.begin(b'D');
        self.buf.push(b'S');
        self.cstr(name);
        self.finish(at)
    }

    pub(crate) fn describe_portal(&mut self, name: &str) -> io::Result<()> {
        let at = self.begin(b'D');
        self.buf.push(b'P');
        self.cstr(name);
        self.finish(at)
    }

    pub(crate) fn execute(&mut self, portal: &str, max_rows: i32) -> io::Result<()> {
        let at = self.begin(b'E');
        self.cstr(portal);
        self.int32(max_rows);
        self.finish(at)
    }

    pub(crate) fn sync(&mut self) -> io::Result<()> {
        let at = self.begin(b'S');
        self.finish(at)
    }

    pub(crate) fn flush(&mut self) -> io::Result<()> {
        let at = self.begin(b'H');
        self.finish(at)
    }

    pub(crate) fn terminate(&mut self) -> io::Result<()> {
        let at = self.begin(b'X');
        self.finish(at)
    }

    pub(crate) fn take(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.buf)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FieldDescription {
    pub(crate) name: String,
    pub(crate) table_oid: u32,
    pub(crate) column: i16,
    pub(crate) type_oid: u32,
    pub(crate) type_modifier: i32,
}

#[derive(Debug)]
pub(crate) enum Backend {
    Authentication { code: i32, data: Vec<u8> },
    ParameterStatus,
    KeyData,
    ReadyForQuery(u8),
    Error(Vec<(u8, String)>),
    Notice,
    ParseComplete,
    BindComplete,
    CloseComplete,
    RowDescription(Vec<FieldDescription>),
    NoData,
    DataRow(Vec<Option<Vec<u8>>>),
    CommandComplete,
    EmptyQuery,
    PortalSuspended,
    ParameterDescription(Vec<u32>),
    Other,
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

struct Reader<'a> {
    body: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn bytes(&mut self, n: usize) -> io::Result<&[u8]> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|end| *end <= self.body.len())
            .ok_or_else(|| invalid("truncated backend message"))?;
        let slice = &self.body[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn int16(&mut self) -> io::Result<i16> {
        let b = self.bytes(2)?;
        Ok(i16::from_be_bytes([b[0], b[1]]))
    }

    fn int32(&mut self) -> io::Result<i32> {
        let b = self.bytes(4)?;
        Ok(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn uint32(&mut self) -> io::Result<u32> {
        let b = self.bytes(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn cstr(&mut self) -> io::Result<String> {
        let rest = &self.body[self.pos..];
        let len = rest
            .iter()
            .position(|b| *b == 0)
            .ok_or_else(|| invalid("unterminated string in backend message"))?;
        let text = String::from_utf8_lossy(&rest[..len]).into_owned();
        self.pos += len + 1;
        Ok(text)
    }

    fn rest(&self) -> Vec<u8> {
        self.body[self.pos..].to_vec()
    }

    fn fields(&mut self) -> io::Result<Vec<(u8, String)>> {
        let mut fields = Vec::new();
        loop {
            let code = self.bytes(1)?[0];
            if code == 0 {
                return Ok(fields);
            }
            fields.push((code, self.cstr()?));
        }
    }
}

fn parse(tag: u8, body: &[u8]) -> io::Result<Backend> {
    let mut r = Reader { body, pos: 0 };
    let message = match tag {
        b'R' => {
            let code = r.int32()?;
            Backend::Authentication {
                code,
                data: r.rest(),
            }
        }
        b'S' => Backend::ParameterStatus,
        b'K' => Backend::KeyData,
        b'Z' => Backend::ReadyForQuery(r.bytes(1)?[0]),
        b'E' => Backend::Error(r.fields()?),
        b'N' => Backend::Notice,
        b'1' => Backend::ParseComplete,
        b'2' => Backend::BindComplete,
        b'3' => Backend::CloseComplete,
        b'T' => {
            let count = usize::try_from(r.int16()?).map_err(|_| invalid("bad field count"))?;
            let mut fields = Vec::with_capacity(count);
            for _ in 0..count {
                let name = r.cstr()?;
                let table_oid = r.uint32()?;
                let column = r.int16()?;
                let type_oid = r.uint32()?;
                let _type_size = r.int16()?;
                let type_modifier = r.int32()?;
                let _format = r.int16()?;
                fields.push(FieldDescription {
                    name,
                    table_oid,
                    column,
                    type_oid,
                    type_modifier,
                });
            }
            Backend::RowDescription(fields)
        }
        b'n' => Backend::NoData,
        b'D' => {
            let count = usize::try_from(r.int16()?).map_err(|_| invalid("bad column count"))?;
            let mut values = Vec::with_capacity(count);
            for _ in 0..count {
                let len = r.int32()?;
                if len < 0 {
                    values.push(None);
                } else {
                    let len = usize::try_from(len).map_err(|_| invalid("bad value length"))?;
                    values.push(Some(r.bytes(len)?.to_vec()));
                }
            }
            Backend::DataRow(values)
        }
        b'C' => Backend::CommandComplete,
        b'I' => Backend::EmptyQuery,
        b's' => Backend::PortalSuspended,
        b't' => {
            let count = usize::try_from(r.int16()?).map_err(|_| invalid("bad parameter count"))?;
            let mut oids = Vec::with_capacity(count);
            for _ in 0..count {
                oids.push(r.uint32()?);
            }
            Backend::ParameterDescription(oids)
        }
        _ => Backend::Other,
    };
    Ok(message)
}

pub(crate) struct Wire {
    stream: BufStream<PgSocket>,
}

impl Wire {
    pub(crate) fn new(socket: PgSocket) -> Wire {
        Wire {
            stream: BufStream::new(socket),
        }
    }

    pub(crate) async fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.stream.write_all(bytes).await?;
        self.stream.flush().await
    }

    pub(crate) async fn receive(&mut self) -> io::Result<Backend> {
        let tag = self.stream.read_u8().await?;
        let len = self.stream.read_i32().await?;
        let body_len = usize::try_from(len - 4).map_err(|_| invalid("bad message length"))?;
        let mut body = vec![0; body_len];
        self.stream.read_exact(&mut body).await?;
        parse(tag, &body)
    }

    pub(crate) async fn shutdown(&mut self) -> io::Result<()> {
        self.stream.shutdown().await
    }
}
