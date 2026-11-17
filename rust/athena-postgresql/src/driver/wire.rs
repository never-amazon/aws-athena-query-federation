use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::{Buf, BytesMut};
use postgres_protocol::message::backend::Message;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;

pub(crate) const MAX_FRAME_BODY: usize = 1 << 30;
const READ_CHUNK: usize = 64 * 1024;
const HEADER_LEN: usize = 5;

pub(crate) enum PgSocket {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl PgSocket {
    pub(crate) fn peer_certificate(&self) -> Option<&[u8]> {
        match self {
            PgSocket::Plain(_) => None,
            PgSocket::Tls(tls) => tls
                .get_ref()
                .1
                .peer_certificates()
                .and_then(|certs| certs.first())
                .map(AsRef::as_ref),
        }
    }
}

impl AsyncRead for PgSocket {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            PgSocket::Plain(s) => Pin::new(s).poll_read(cx, buf),
            PgSocket::Tls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for PgSocket {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            PgSocket::Plain(s) => Pin::new(s).poll_write(cx, buf),
            PgSocket::Tls(s) => Pin::new(s.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            PgSocket::Plain(s) => Pin::new(s).poll_flush(cx),
            PgSocket::Tls(s) => Pin::new(s.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            PgSocket::Plain(s) => Pin::new(s).poll_shutdown(cx),
            PgSocket::Tls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
        }
    }
}

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub(crate) fn frame_length(header: &[u8]) -> io::Result<Option<usize>> {
    let Some(len) = header.get(1..HEADER_LEN) else {
        return Ok(None);
    };
    let len = i32::from_be_bytes([len[0], len[1], len[2], len[3]]);
    let body = usize::try_from(len)
        .ok()
        .and_then(|len| len.checked_sub(4))
        .ok_or_else(|| invalid(format!("Invalid message length {len}")))?;
    if body > MAX_FRAME_BODY {
        return Err(invalid(format!(
            "Message length {len} exceeds the limit of {MAX_FRAME_BODY} bytes"
        )));
    }
    Ok(Some(body + HEADER_LEN))
}

pub(crate) struct Wire {
    socket: PgSocket,
    input: BytesMut,
    output: BytesMut,
}

impl Wire {
    pub(crate) fn new(socket: PgSocket) -> Wire {
        Wire {
            socket,
            input: BytesMut::with_capacity(READ_CHUNK),
            output: BytesMut::new(),
        }
    }

    pub(crate) fn socket(&self) -> &PgSocket {
        &self.socket
    }

    pub(crate) fn output(&mut self) -> &mut BytesMut {
        &mut self.output
    }

    pub(crate) async fn flush(&mut self) -> io::Result<()> {
        while !self.output.is_empty() {
            let written = self.socket.write(&self.output).await?;
            if written == 0 {
                return Err(io::Error::from(io::ErrorKind::WriteZero));
            }
            self.output.advance(written);
        }
        self.socket.flush().await
    }

    pub(crate) async fn receive(&mut self) -> io::Result<Message> {
        self.flush().await?;
        loop {
            if let Some(total) = frame_length(&self.input)? {
                if self.input.len() >= total {
                    return Message::parse(&mut self.input)?
                        .ok_or_else(|| invalid("Incomplete message from the backend".to_string()));
                }
                self.input
                    .reserve((total - self.input.len()).min(READ_CHUNK));
            } else {
                self.input.reserve(READ_CHUNK);
            }
            if self.socket.read_buf(&mut self.input).await? == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "EOF Exception",
                ));
            }
        }
    }

    pub(crate) async fn shutdown(&mut self) -> io::Result<()> {
        self.socket.shutdown().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_length_rejects_short_negative_and_huge_lengths() {
        assert_eq!(frame_length(b"Z\0\0").unwrap(), None);
        assert_eq!(frame_length(b"Z\0\0\0\x05I").unwrap(), Some(6));
        assert!(frame_length(b"R\x80\0\0\0").is_err());
        assert!(frame_length(b"R\0\0\0\x03").is_err());
        assert!(frame_length(b"R\x7f\xff\xff\xf0").is_err());
        assert!(frame_length(b"R\x40\0\0\x04").unwrap().is_some());
        assert!(frame_length(b"R\x40\0\0\x05").is_err());
    }
}
