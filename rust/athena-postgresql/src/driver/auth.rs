use std::io;

use athena_jdbc::sql::SqlException;
use fallible_iterator::FallibleIterator;
use postgres_protocol::authentication::md5_hash;
use postgres_protocol::authentication::sasl::{
    ChannelBinding as ScramBinding, ScramSha256, SCRAM_SHA_256, SCRAM_SHA_256_PLUS,
};
use postgres_protocol::message::backend::Message;
use postgres_protocol::message::frontend;

use super::error::{
    io_failure, protocol_violation, psql, CONNECTION_REJECTED, INVALID_PARAMETER_VALUE,
    PROTOCOL_VIOLATION,
};
use super::session::{BackendKey, Session};
use super::tls::tls_server_end_point;
use super::url::{AuthMethod, ChannelBinding, ConnectSettings};

#[derive(Debug)]
pub(crate) enum StartupError {
    Sql(SqlException),
    Io(io::Error),
}

impl From<SqlException> for StartupError {
    fn from(error: SqlException) -> StartupError {
        StartupError::Sql(error)
    }
}

impl From<io::Error> for StartupError {
    fn from(error: io::Error) -> StartupError {
        StartupError::Io(error)
    }
}

fn rejected(message: impl Into<String>) -> StartupError {
    StartupError::Sql(psql(message, CONNECTION_REJECTED))
}

fn method_name(code: i32) -> String {
    match code {
        0 => "none".to_string(),
        3 => "password".to_string(),
        5 => "md5".to_string(),
        7 => "gss".to_string(),
        9 => "sspi".to_string(),
        other => other.to_string(),
    }
}

fn check_auth(allowed: Option<&[AuthMethod]>, method: AuthMethod) -> Result<(), StartupError> {
    match allowed {
        Some(allowed) if !allowed.contains(&method) => Err(rejected(
            "Authentication method is not allowed by requireAuth",
        )),
        _ => Ok(()),
    }
}

fn unsupported(code: i32) -> StartupError {
    rejected(format!(
        "The authentication type {code} is not supported. Check that you have configured the pg_hba.conf file to include the client's IP address or subnet, and that it is using an authentication scheme supported by the driver."
    ))
}

fn no_password() -> StartupError {
    rejected(
        "The server requested password-based authentication, but no password was provided by plugin null",
    )
}

fn scram_failed(message: impl std::fmt::Display) -> StartupError {
    rejected(format!("SCRAM authentication failed: {message}"))
}

fn grouped(value: i64) -> String {
    let digits = value.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if value < 0 {
        out.push('-');
    }
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub(crate) fn server_iterations(server_first: &[u8]) -> Option<i64> {
    std::str::from_utf8(server_first)
        .ok()?
        .split(',')
        .find_map(|part| part.strip_prefix("i="))
        .and_then(|i| i.parse().ok())
}

pub(crate) fn check_iterations(
    server_first: &[u8],
    max_iterations: i64,
) -> Result<(), StartupError> {
    let Some(iterations) = server_iterations(server_first) else {
        return Err(scram_failed("invalid iteration"));
    };
    if iterations <= 0 {
        return Err(scram_failed(format!(
            "'iterationCount' must be positive, was: {iterations}"
        )));
    }
    if max_iterations > 0 && iterations > max_iterations {
        return Err(rejected(format!(
            "Server requested {} SCRAM PBKDF2 iterations, which exceeds the client-side limit of {}. If you trust this server, raise the scramMaxIterations connection property.",
            grouped(iterations),
            grouped(max_iterations)
        )));
    }
    Ok(())
}

struct ScramState {
    client: ScramSha256,
    finished: bool,
}

fn start_scram(
    mechanisms: &[String],
    password: &str,
    channel_binding: ChannelBinding,
    peer_certificate: Option<&[u8]>,
) -> Result<(&'static str, ScramSha256), StartupError> {
    if mechanisms.is_empty() {
        return Err(rejected(
            "Received AuthenticationSASL message with 0 mechanisms!",
        ));
    }
    if channel_binding == ChannelBinding::Require
        && !mechanisms.iter().any(|m| m.ends_with("-PLUS"))
    {
        return Err(rejected(
            "Channel Binding is required, but server did not offer an authentication method that supports channel binding",
        ));
    }
    let cbind_data = match (channel_binding, peer_certificate) {
        (ChannelBinding::Require, None) => {
            return Err(rejected(
                "Channel Binding is required, but SSL is not in use",
            ))
        }
        (ChannelBinding::Prefer | ChannelBinding::Require, Some(cert)) => {
            tls_server_end_point(cert)
        }
        _ => None,
    };
    if channel_binding == ChannelBinding::Require && cbind_data.is_none() {
        return Err(rejected(
            "Channel Binding is required, but could not extract channel binding data from SSL session",
        ));
    }
    let plus_offered = mechanisms.iter().any(|m| m == SCRAM_SHA_256_PLUS);
    let bare_offered = mechanisms.iter().any(|m| m == SCRAM_SHA_256);
    let (mechanism, binding) = match cbind_data {
        Some(data) if plus_offered => {
            (SCRAM_SHA_256_PLUS, ScramBinding::tls_server_end_point(data))
        }
        Some(_) if bare_offered => (SCRAM_SHA_256, ScramBinding::unrequested()),
        None if bare_offered => (SCRAM_SHA_256, ScramBinding::unsupported()),
        _ => return Err(rejected("Invalid SCRAM client initialization")),
    };
    if channel_binding == ChannelBinding::Require && mechanism != SCRAM_SHA_256_PLUS {
        return Err(rejected(format!(
            "Channel Binding is required, but the negotiated SCRAM mechanism \"{mechanism}\" does not use channel binding."
        )));
    }
    Ok((mechanism, ScramSha256::new(password.as_bytes(), binding)))
}

fn sasl_error(error: &io::Error) -> String {
    let text = error.to_string();
    if text == "SCRAM verification error" {
        "Invalid SCRAM server signature".to_string()
    } else {
        text
    }
}

async fn flush(session: &mut Session) -> Result<(), StartupError> {
    session.wire_mut()?.flush().await.map_err(StartupError::Io)
}

fn auth_code(session: &Session, message: &Message) -> Result<i32, StartupError> {
    Ok(match message {
        Message::AuthenticationOk => 0,
        Message::AuthenticationKerberosV5 => 2,
        Message::AuthenticationCleartextPassword => 3,
        Message::AuthenticationMd5Password(_) => 5,
        Message::AuthenticationScmCredential => 6,
        Message::AuthenticationGss => 7,
        Message::AuthenticationGssContinue(_) => 8,
        Message::AuthenticationSspi => 9,
        Message::AuthenticationSasl(_) => 10,
        Message::AuthenticationSaslContinue(_) => 11,
        Message::AuthenticationSaslFinal(_) => 12,
        Message::ErrorResponse(body) => {
            return Err(StartupError::Sql(session.server_error(body.fields())))
        }
        _ => {
            return Err(StartupError::Sql(psql(
                "Protocol error.  Session setup failed.",
                PROTOCOL_VIOLATION,
            )))
        }
    })
}

struct Authenticator<'a> {
    settings: &'a ConnectSettings,
    scram: Option<ScramState>,
    finished_requests: bool,
}

impl Authenticator<'_> {
    fn scram_finished(&self) -> bool {
        self.scram.as_ref().is_some_and(|s| s.finished)
    }

    fn check_channel_binding(&self, code: i32) -> Result<(), StartupError> {
        if self.settings.channel_binding != ChannelBinding::Require {
            return Ok(());
        }
        if code == 0 && !self.scram_finished() {
            return Err(rejected(
                "Channel binding is required, but server skipped authentication. Channel binding is only supported with SCRAM authentication over encrypted connections.",
            ));
        }
        if code != 0 && !(10..=12).contains(&code) {
            return Err(rejected(format!(
                "Channel binding is required, but server requested '{}' authentication. Channel binding is only supported with SCRAM authentication over encrypted connections.",
                method_name(code)
            )));
        }
        Ok(())
    }

    fn allowed(&self) -> Option<&[AuthMethod]> {
        self.settings.require_auth.as_deref()
    }

    async fn send_password(
        &mut self,
        session: &mut Session,
        password: &[u8],
    ) -> Result<(), StartupError> {
        frontend::password_message(password, session.output()?).map_err(io_failure)?;
        flush(session).await?;
        self.finished_requests = true;
        Ok(())
    }

    async fn sasl_start(
        &mut self,
        session: &mut Session,
        mechanisms: Vec<String>,
    ) -> Result<(), StartupError> {
        check_auth(self.allowed(), AuthMethod::ScramSha256)?;
        if self.settings.scram_max_iterations < 0 {
            return Err(StartupError::Sql(psql(
                format!(
                    "scramMaxIterations must be a non-negative integer, but was: {}",
                    grouped(self.settings.scram_max_iterations)
                ),
                INVALID_PARAMETER_VALUE,
            )));
        }
        let password = match self.settings.password.as_deref() {
            None => {
                return Err(rejected(
                    "The server requested SCRAM-based authentication, but no password was provided.",
                ))
            }
            Some("") => {
                return Err(rejected(
                    "The server requested SCRAM-based authentication, but the password is an empty string.",
                ))
            }
            Some(p) => p,
        };
        let peer = session
            .wire_ref()
            .and_then(|w| w.socket().peer_certificate())
            .map(<[u8]>::to_vec);
        let (mechanism, client) = start_scram(
            &mechanisms,
            password,
            self.settings.channel_binding,
            peer.as_deref(),
        )?;
        frontend::sasl_initial_response(mechanism, client.message(), session.output()?)
            .map_err(io_failure)?;
        flush(session).await?;
        self.scram = Some(ScramState {
            client,
            finished: false,
        });
        Ok(())
    }

    async fn sasl_continue(
        &mut self,
        session: &mut Session,
        data: &[u8],
    ) -> Result<(), StartupError> {
        let state = self.scram.as_mut().ok_or_else(|| {
            StartupError::Sql(protocol_violation("Unexpected SASL continue message"))
        })?;
        check_iterations(data, self.settings.scram_max_iterations)?;
        state
            .client
            .update(data)
            .map_err(|e| scram_failed(sasl_error(&e)))?;
        frontend::sasl_response(state.client.message(), session.output()?).map_err(io_failure)?;
        flush(session).await
    }

    fn sasl_final(&mut self, data: &[u8]) -> Result<(), StartupError> {
        let state = self.scram.as_mut().ok_or_else(|| {
            StartupError::Sql(protocol_violation("Unexpected SASL final message"))
        })?;
        state
            .client
            .finish(data)
            .map_err(|e| scram_failed(sasl_error(&e)))?;
        state.finished = true;
        self.finished_requests = true;
        Ok(())
    }

    async fn handle(
        &mut self,
        session: &mut Session,
        message: Message,
        code: i32,
    ) -> Result<bool, StartupError> {
        let user = self.settings.user.as_deref().unwrap_or_default();
        let password = self.settings.password.as_deref();
        match message {
            Message::AuthenticationOk => {
                if self.scram.as_ref().is_some_and(|s| !s.finished) {
                    return Err(scram_failed(
                        "the server sent AuthenticationOk before the SCRAM exchange finished",
                    ));
                }
                if !self.finished_requests {
                    check_auth(self.allowed(), AuthMethod::None)?;
                }
                return Ok(true);
            }
            Message::AuthenticationCleartextPassword => {
                check_auth(self.allowed(), AuthMethod::Password)?;
                let password = password.ok_or_else(no_password)?;
                self.send_password(session, password.as_bytes()).await?;
            }
            Message::AuthenticationMd5Password(body) => {
                check_auth(self.allowed(), AuthMethod::Md5)?;
                let password = password.ok_or_else(no_password)?;
                let hashed = md5_hash(user.as_bytes(), password.as_bytes(), body.salt());
                self.send_password(session, hashed.as_bytes()).await?;
            }
            Message::AuthenticationGss => {
                check_auth(self.allowed(), AuthMethod::Gss)?;
                return Err(unsupported(code));
            }
            Message::AuthenticationSspi => {
                check_auth(self.allowed(), AuthMethod::Sspi)?;
                return Err(unsupported(code));
            }
            Message::AuthenticationSasl(body) => {
                let mechanisms: Vec<String> = body
                    .mechanisms()
                    .map(|m| Ok(m.to_string()))
                    .collect()
                    .map_err(|e| {
                        StartupError::Sql(protocol_violation(format!("Invalid SASL message: {e}")))
                    })?;
                self.sasl_start(session, mechanisms).await?;
            }
            Message::AuthenticationSaslContinue(body) => {
                self.sasl_continue(session, body.data()).await?;
            }
            Message::AuthenticationSaslFinal(body) => self.sasl_final(body.data())?,
            _ => return Err(unsupported(code)),
        }
        Ok(false)
    }
}

pub(crate) async fn authenticate(
    session: &mut Session,
    settings: &ConnectSettings,
) -> Result<(), StartupError> {
    let mut authenticator = Authenticator {
        settings,
        scram: None,
        finished_requests: false,
    };
    loop {
        let message = session.receive_io().await?;
        let code = auth_code(session, &message)?;
        authenticator.check_channel_binding(code)?;
        if authenticator.handle(session, message, code).await? {
            break;
        }
    }
    read_startup_messages(session).await
}

async fn read_startup_messages(session: &mut Session) -> Result<(), StartupError> {
    loop {
        match session.receive_io().await? {
            Message::BackendKeyData(body) => session.set_backend_key(BackendKey {
                process_id: body.process_id(),
                secret_key: body.secret_key(),
            }),
            Message::ErrorResponse(body) => {
                return Err(StartupError::Sql(session.server_error(body.fields())))
            }
            Message::ReadyForQuery(body) => {
                session.set_tx_status(body.status());
                return Ok(());
            }
            Message::ParameterStatus(_) | Message::NoticeResponse(_) => {}
            _ => {
                return Err(StartupError::Sql(psql(
                    "Protocol error.  Session setup failed.",
                    PROTOCOL_VIOLATION,
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(error: StartupError) -> String {
        match error {
            StartupError::Sql(e) => e.message().to_string(),
            StartupError::Io(e) => e.to_string(),
        }
    }

    #[test]
    fn iteration_limits_match_pgjdbc() {
        assert!(check_iterations(b"r=abc,s=c2FsdA==,i=4096", 100_000).is_ok());
        assert_eq!(
            message(check_iterations(b"r=abc,s=c2FsdA==,i=100001", 100_000).unwrap_err()),
            "Server requested 100,001 SCRAM PBKDF2 iterations, which exceeds the client-side limit of 100,000. If you trust this server, raise the scramMaxIterations connection property."
        );
        assert_eq!(
            message(check_iterations(b"r=abc,s=c2FsdA==,i=0", 100_000).unwrap_err()),
            "SCRAM authentication failed: 'iterationCount' must be positive, was: 0"
        );
        assert!(check_iterations(b"r=abc,s=c2FsdA==,i=200000", 0).is_ok());
    }

    #[test]
    fn channel_binding_choice_follows_pgjdbc() {
        let both = vec![SCRAM_SHA_256_PLUS.to_string(), SCRAM_SHA_256.to_string()];
        let bare = vec![SCRAM_SHA_256.to_string()];
        let cert = [
            0x30, 0x0a, 0x30, 0x00, 0x30, 0x03, 0x06, 0x01, 0x2a, 0x03, 0x01, 0x00,
        ];
        let (m, _) = start_scram(&both, "pw", ChannelBinding::Prefer, Some(&cert)).unwrap();
        assert_eq!(m, SCRAM_SHA_256_PLUS);
        let (m, c) = start_scram(&bare, "pw", ChannelBinding::Prefer, Some(&cert)).unwrap();
        assert_eq!(m, SCRAM_SHA_256);
        assert!(c.message().starts_with(b"y,,"));
        let (m, c) = start_scram(&both, "pw", ChannelBinding::Prefer, None).unwrap();
        assert_eq!(m, SCRAM_SHA_256);
        assert!(c.message().starts_with(b"n,,"));
        let (_, c) = start_scram(&both, "pw", ChannelBinding::Disable, Some(&cert)).unwrap();
        assert!(c.message().starts_with(b"n,,"));
        assert_eq!(
            message(start_scram(&bare, "pw", ChannelBinding::Require, Some(&cert)).err().unwrap()),
            "Channel Binding is required, but server did not offer an authentication method that supports channel binding"
        );
        assert_eq!(
            message(
                start_scram(&both, "pw", ChannelBinding::Require, None)
                    .err()
                    .unwrap()
            ),
            "Channel Binding is required, but SSL is not in use"
        );
    }
}
