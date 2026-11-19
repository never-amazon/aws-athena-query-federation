use std::collections::HashMap;
use std::time::Duration;

use athena_jdbc::sql::SqlError;

use super::codec::hex_digit;
use super::error::{psql, CONNECTION_UNABLE_TO_CONNECT, INVALID_PARAMETER_VALUE};

const URL_PREFIX: &str = "jdbc:postgresql:";
const DEFAULT_PORT: u16 = 5432;
const DEFAULT_CONNECT_TIMEOUT_SECONDS: u64 = 10;
const DEFAULT_SSL_RESPONSE_TIMEOUT_MILLIS: u64 = 5000;
const DEFAULT_CANCEL_SIGNAL_TIMEOUT_SECONDS: u64 = 10;
const DEFAULT_SCRAM_MAX_ITERATIONS: i64 = 100_000;
const DEFAULT_APPLICATION_NAME: &str = "PostgreSQL JDBC Driver";
const LIBPQ_FACTORIES: [&str; 2] = [
    "org.postgresql.ssl.LibPQFactory",
    "org.postgresql.ssl.jdbc4.LibPQFactory",
];
const NON_VALIDATING_FACTORY: &str = "org.postgresql.ssl.NonValidatingFactory";
const DEFAULT_JAVA_FACTORY: &str = "org.postgresql.ssl.DefaultJavaSSLFactory";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SslMode {
    Disable,
    Allow,
    Prefer,
    Require,
    VerifyCa,
    VerifyFull,
}

impl SslMode {
    const VALUES: [(&'static str, SslMode); 6] = [
        ("disable", SslMode::Disable),
        ("allow", SslMode::Allow),
        ("prefer", SslMode::Prefer),
        ("require", SslMode::Require),
        ("verify-ca", SslMode::VerifyCa),
        ("verify-full", SslMode::VerifyFull),
    ];

    fn of(properties: &HashMap<String, String>) -> Result<SslMode, SqlError> {
        let Some(value) = properties.get("sslmode") else {
            let ssl = properties.get("ssl").map(String::as_str);
            return Ok(match ssl {
                Some("") => SslMode::VerifyFull,
                Some(v) if v.eq_ignore_ascii_case("true") => SslMode::VerifyFull,
                _ => SslMode::Prefer,
            });
        };
        SslMode::VALUES
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(value))
            .map(|(_, mode)| *mode)
            .ok_or_else(|| {
                psql(
                    format!("Invalid sslmode value: {value}"),
                    CONNECTION_UNABLE_TO_CONNECT,
                )
            })
    }

    pub(crate) fn require_encryption(self) -> bool {
        self >= SslMode::Require
    }

    pub(crate) fn verify_certificate(self) -> bool {
        matches!(self, SslMode::VerifyCa | SslMode::VerifyFull)
    }

    pub(crate) fn verify_peer_name(self) -> bool {
        self == SslMode::VerifyFull
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SslFactory {
    LibPq,
    NonValidating,
    DefaultJava,
    Unsupported(String),
}

impl SslFactory {
    fn of(properties: &HashMap<String, String>) -> SslFactory {
        match properties.get("sslfactory").map(String::as_str) {
            None => SslFactory::LibPq,
            Some(name) if LIBPQ_FACTORIES.contains(&name) => SslFactory::LibPq,
            Some(NON_VALIDATING_FACTORY) => SslFactory::NonValidating,
            Some(DEFAULT_JAVA_FACTORY) => SslFactory::DefaultJava,
            Some(other) => SslFactory::Unsupported(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SslNegotiation {
    Postgres,
    Direct,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChannelBinding {
    Disable,
    Prefer,
    Require,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum AuthMethod {
    None,
    Password,
    Md5,
    Gss,
    Sspi,
    ScramSha256,
}

impl AuthMethod {
    const ALL: [AuthMethod; 6] = [
        AuthMethod::None,
        AuthMethod::Password,
        AuthMethod::Md5,
        AuthMethod::Gss,
        AuthMethod::Sspi,
        AuthMethod::ScramSha256,
    ];

    fn from_name(name: &str) -> Result<AuthMethod, SqlError> {
        match name {
            "none" => Ok(AuthMethod::None),
            "password" => Ok(AuthMethod::Password),
            "md5" => Ok(AuthMethod::Md5),
            "gss" => Ok(AuthMethod::Gss),
            "sspi" => Ok(AuthMethod::Sspi),
            "scram-sha-256" => Ok(AuthMethod::ScramSha256),
            other => Err(psql(
                format!("Invalid authentication method: {other}"),
                INVALID_PARAMETER_VALUE,
            )),
        }
    }
}

pub(crate) fn parse_require_auth(text: &str) -> Result<Option<Vec<AuthMethod>>, SqlError> {
    let methods: Vec<&str> = text.split(',').map(str::trim).collect();
    let disallow = methods.first().is_some_and(|m| m.starts_with('!'));
    let mut allowed: Vec<AuthMethod> = if disallow {
        AuthMethod::ALL.to_vec()
    } else {
        Vec::new()
    };
    let mut seen = Vec::new();
    for method in methods {
        let negative = method.starts_with('!');
        if negative != disallow {
            return Err(psql(
                "requireAuth cannot mix positive and negative authentication methods",
                INVALID_PARAMETER_VALUE,
            ));
        }
        let parsed = AuthMethod::from_name(method.strip_prefix('!').unwrap_or(method))?;
        if seen.contains(&parsed) {
            return Err(psql(
                "requireAuth contains duplicate authentication method",
                INVALID_PARAMETER_VALUE,
            ));
        }
        seen.push(parsed);
        if disallow {
            allowed.retain(|m| *m != parsed);
        } else {
            allowed.push(parsed);
        }
    }
    Ok((!allowed.is_empty()).then_some(allowed))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HostSpec {
    pub(crate) host: String,
    pub(crate) port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConnectSettings {
    pub(crate) hosts: Vec<HostSpec>,
    pub(crate) database: String,
    pub(crate) user: Option<String>,
    pub(crate) password: Option<String>,
    pub(crate) ssl_mode: SslMode,
    pub(crate) ssl_factory: SslFactory,
    pub(crate) ssl_negotiation: SslNegotiation,
    pub(crate) ssl_root_cert: Option<String>,
    pub(crate) ssl_response_timeout: Option<Duration>,
    pub(crate) channel_binding: ChannelBinding,
    pub(crate) require_auth: Option<Vec<AuthMethod>>,
    pub(crate) scram_max_iterations: i64,
    pub(crate) connect_timeout: Option<Duration>,
    pub(crate) socket_timeout: Option<Duration>,
    pub(crate) login_timeout: Option<Duration>,
    pub(crate) cancel_signal_timeout: Option<Duration>,
    pub(crate) tcp_keep_alive: bool,
    pub(crate) application_name: String,
    pub(crate) current_schema: Option<String>,
    pub(crate) options: Option<String>,
    pub(crate) string_type_unspecified: bool,
    pub(crate) log_server_error_detail: bool,
    pub(crate) time_zone: String,
}

fn bad_url() -> SqlError {
    psql(
        "No suitable driver found for the given URL",
        CONNECTION_UNABLE_TO_CONNECT,
    )
}

pub(crate) fn url_decode(text: &str) -> Result<String, SqlError> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' => {
                let decoded = bytes
                    .get(i + 1)
                    .and_then(|h| hex_digit(*h))
                    .zip(bytes.get(i + 2).and_then(|l| hex_digit(*l)))
                    .ok_or_else(|| {
                        psql(
                            format!("Unable to decode URL entry via UTF-8: {text}"),
                            INVALID_PARAMETER_VALUE,
                        )
                    })?;
                out.push(decoded.0 * 16 + decoded.1);
                i += 2;
            }
            other => out.push(other),
        }
        i += 1;
    }
    String::from_utf8(out).map_err(|_| {
        psql(
            format!("Unable to decode URL entry via UTF-8: {text}"),
            INVALID_PARAMETER_VALUE,
        )
    })
}

fn parse_host(spec: &str) -> Result<HostSpec, SqlError> {
    let (host, port) = if let Some(rest) = spec.strip_prefix('[') {
        let end = rest.find(']').ok_or_else(bad_url)?;
        let host = &rest[..end];
        let port = rest[end + 1..].strip_prefix(':');
        (host, port)
    } else {
        match spec.rfind(':') {
            Some(at) => (&spec[..at], Some(&spec[at + 1..])),
            None => (spec, None),
        }
    };
    let port = match port {
        None | Some("") => DEFAULT_PORT,
        Some(p) => p.parse::<u16>().map_err(|_| bad_url())?,
    };
    Ok(HostSpec {
        host: if host.is_empty() {
            "localhost".to_string()
        } else {
            url_decode(host)?
        },
        port,
    })
}

pub(crate) fn parse_url(
    url: &str,
    info: &HashMap<String, String>,
) -> Result<ConnectSettings, SqlError> {
    let rest = url.strip_prefix(URL_PREFIX).ok_or_else(bad_url)?;
    let (address, query) = match rest.find('?') {
        Some(at) => (&rest[..at], &rest[at + 1..]),
        None => (rest, ""),
    };
    let mut properties: HashMap<String, String> = info.clone();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = match pair.find('=') {
            Some(at) => (&pair[..at], &pair[at + 1..]),
            None => (pair, ""),
        };
        properties.insert(key.to_string(), url_decode(value)?);
    }
    let (hosts, database) = if let Some(after) = address.strip_prefix("//") {
        let (host_part, database) = match after.find('/') {
            Some(at) => (&after[..at], &after[at + 1..]),
            None => (after, ""),
        };
        let hosts = host_part
            .split(',')
            .map(parse_host)
            .collect::<Result<Vec<_>, _>>()?;
        (hosts, url_decode(database)?)
    } else {
        (
            vec![HostSpec {
                host: "localhost".to_string(),
                port: DEFAULT_PORT,
            }],
            url_decode(address)?,
        )
    };
    settings_from(hosts, database, &properties)
}

fn number(properties: &HashMap<String, String>, key: &str, default: i64) -> i64 {
    properties
        .get(key)
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(default)
}

fn seconds(properties: &HashMap<String, String>, key: &str, default: u64) -> Option<Duration> {
    let value = properties
        .get(key)
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(default);
    (value > 0).then(|| Duration::from_secs(value))
}

fn login_timeout(properties: &HashMap<String, String>) -> Option<Duration> {
    properties
        .get("loginTimeout")
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v > 0.0)
        .and_then(|v| Duration::try_from_secs_f64(v).ok())
}

fn flag(properties: &HashMap<String, String>, key: &str, default: bool) -> bool {
    properties
        .get(key)
        .map_or(default, |v| v.eq_ignore_ascii_case("true"))
}

pub(crate) fn default_time_zone() -> String {
    std::env::var("TZ")
        .ok()
        .map(|tz| tz.trim_start_matches(':').to_string())
        .filter(|tz| !tz.is_empty())
        .unwrap_or_else(|| "UTC".to_string())
}

fn channel_binding(properties: &HashMap<String, String>) -> Result<ChannelBinding, SqlError> {
    match properties
        .get("channelBinding")
        .map_or("prefer", String::as_str)
    {
        "disable" => Ok(ChannelBinding::Disable),
        "prefer" => Ok(ChannelBinding::Prefer),
        "require" => Ok(ChannelBinding::Require),
        other => Err(psql(
            format!("Invalid channelBinding value: {other}"),
            CONNECTION_UNABLE_TO_CONNECT,
        )),
    }
}

fn ssl_negotiation(properties: &HashMap<String, String>) -> Result<SslNegotiation, SqlError> {
    match properties
        .get("sslNegotiation")
        .map_or("postgres", String::as_str)
    {
        "postgres" => Ok(SslNegotiation::Postgres),
        "direct" => Ok(SslNegotiation::Direct),
        other => Err(psql(
            format!("Invalid sslNegotiation value: {other}"),
            CONNECTION_UNABLE_TO_CONNECT,
        )),
    }
}

fn settings_from(
    hosts: Vec<HostSpec>,
    database: String,
    properties: &HashMap<String, String>,
) -> Result<ConnectSettings, SqlError> {
    let ssl_response_millis = u64::try_from(number(
        properties,
        "sslResponseTimeout",
        i64::try_from(DEFAULT_SSL_RESPONSE_TIMEOUT_MILLIS).unwrap_or(i64::MAX),
    ))
    .unwrap_or(0);
    Ok(ConnectSettings {
        hosts,
        database,
        user: properties.get("user").cloned(),
        password: properties.get("password").cloned(),
        ssl_mode: SslMode::of(properties)?,
        ssl_factory: SslFactory::of(properties),
        ssl_negotiation: ssl_negotiation(properties)?,
        ssl_root_cert: properties.get("sslrootcert").cloned(),
        ssl_response_timeout: (ssl_response_millis > 0)
            .then(|| Duration::from_millis(ssl_response_millis)),
        channel_binding: channel_binding(properties)?,
        require_auth: properties
            .get("requireAuth")
            .map(|v| parse_require_auth(v))
            .transpose()?
            .flatten(),
        scram_max_iterations: number(
            properties,
            "scramMaxIterations",
            DEFAULT_SCRAM_MAX_ITERATIONS,
        ),
        connect_timeout: seconds(
            properties,
            "connectTimeout",
            DEFAULT_CONNECT_TIMEOUT_SECONDS,
        ),
        socket_timeout: seconds(properties, "socketTimeout", 0),
        login_timeout: login_timeout(properties),
        cancel_signal_timeout: seconds(
            properties,
            "cancelSignalTimeout",
            DEFAULT_CANCEL_SIGNAL_TIMEOUT_SECONDS,
        ),
        tcp_keep_alive: flag(properties, "tcpKeepAlive", false),
        application_name: properties
            .get("ApplicationName")
            .or_else(|| properties.get("applicationName"))
            .cloned()
            .unwrap_or_else(|| DEFAULT_APPLICATION_NAME.to_string()),
        current_schema: properties.get("currentSchema").cloned(),
        options: properties.get("options").cloned(),
        string_type_unspecified: properties
            .get("stringtype")
            .is_some_and(|v| v.eq_ignore_ascii_case("unspecified")),
        log_server_error_detail: flag(properties, "logServerErrorDetail", true),
        time_zone: default_time_zone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_host_port_database_and_parameters() {
        let settings = parse_url(
            "jdbc:postgresql://db.example.com:6543/sales?user=alice&password=p%40ss&sslmode=require",
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(
            settings.hosts,
            vec![HostSpec {
                host: "db.example.com".to_string(),
                port: 6543
            }]
        );
        assert_eq!(settings.database, "sales");
        assert_eq!(settings.user.as_deref(), Some("alice"));
        assert_eq!(settings.password.as_deref(), Some("p@ss"));
        assert_eq!(settings.ssl_mode, SslMode::Require);
        assert_eq!(settings.ssl_factory, SslFactory::LibPq);
    }

    #[test]
    fn url_parameters_override_properties() {
        let info = HashMap::from([
            ("user".to_string(), "from-info".to_string()),
            ("password".to_string(), "secret".to_string()),
        ]);
        let settings = parse_url("jdbc:postgresql://h/db?user=from-url", &info).unwrap();
        assert_eq!(settings.user.as_deref(), Some("from-url"));
        assert_eq!(settings.password.as_deref(), Some("secret"));
        assert_eq!(settings.hosts[0].port, 5432);
    }

    #[test]
    fn empty_query_after_secret_removal_is_accepted() {
        let settings = parse_url(
            "jdbc:postgresql://127.0.0.1:5555/paritydb?",
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(settings.database, "paritydb");
        assert_eq!(settings.ssl_mode, SslMode::Prefer);
        assert_eq!(settings.connect_timeout, Some(Duration::from_secs(10)));
    }

    #[test]
    fn multiple_hosts_and_ipv6_are_parsed() {
        let settings = parse_url("jdbc:postgresql://a:1,[::1]:2,b/db", &HashMap::new()).unwrap();
        let hosts: Vec<(String, u16)> = settings
            .hosts
            .into_iter()
            .map(|h| (h.host, h.port))
            .collect();
        assert_eq!(
            hosts,
            vec![
                ("a".to_string(), 1),
                ("::1".to_string(), 2),
                ("b".to_string(), 5432)
            ]
        );
    }

    #[test]
    fn rejects_other_url_schemes() {
        assert!(parse_url("jdbc:mysql://h/db", &HashMap::new()).is_err());
    }
}

#[cfg(test)]
mod ssl_tests {
    use super::*;

    fn mode(url: &str) -> Result<SslMode, SqlError> {
        parse_url(url, &HashMap::new()).map(|s| s.ssl_mode)
    }

    #[test]
    fn bare_or_empty_ssl_means_verify_full() {
        assert_eq!(
            mode("jdbc:postgresql://h/db?ssl").unwrap(),
            SslMode::VerifyFull
        );
        assert_eq!(
            mode("jdbc:postgresql://h/db?ssl=").unwrap(),
            SslMode::VerifyFull
        );
        assert_eq!(
            mode("jdbc:postgresql://h/db?ssl=TRUE").unwrap(),
            SslMode::VerifyFull
        );
        assert_eq!(
            mode("jdbc:postgresql://h/db?ssl=false").unwrap(),
            SslMode::Prefer
        );
        assert_eq!(
            mode("jdbc:postgresql://h/db?ssl=yes").unwrap(),
            SslMode::Prefer
        );
        assert_eq!(mode("jdbc:postgresql://h/db").unwrap(), SslMode::Prefer);
    }

    #[test]
    fn sslmode_is_case_insensitive_and_wins_over_ssl() {
        assert_eq!(
            mode("jdbc:postgresql://h/db?sslmode=REQUIRE").unwrap(),
            SslMode::Require
        );
        assert_eq!(
            mode("jdbc:postgresql://h/db?sslmode=Verify-Full").unwrap(),
            SslMode::VerifyFull
        );
        assert_eq!(
            mode("jdbc:postgresql://h/db?ssl&sslmode=disable").unwrap(),
            SslMode::Disable
        );
        let error = mode("jdbc:postgresql://h/db?sslmode=strict").unwrap_err();
        assert_eq!(error.message(), "Invalid sslmode value: strict");
        assert_eq!(error.sql_state(), Some("08001"));
    }

    #[test]
    fn require_auth_follows_pgjdbc_rules() {
        assert_eq!(
            parse_require_auth("scram-sha-256").unwrap(),
            Some(vec![AuthMethod::ScramSha256])
        );
        let denied = parse_require_auth("!password,!md5").unwrap().unwrap();
        assert!(!denied.contains(&AuthMethod::Password));
        assert!(denied.contains(&AuthMethod::ScramSha256));
        assert_eq!(
            parse_require_auth("md5,!password").unwrap_err().message(),
            "requireAuth cannot mix positive and negative authentication methods"
        );
        assert_eq!(
            parse_require_auth("md5,md5").unwrap_err().message(),
            "requireAuth contains duplicate authentication method"
        );
        assert_eq!(
            parse_require_auth("kerberos").unwrap_err().message(),
            "Invalid authentication method: kerberos"
        );
    }

    #[test]
    fn channel_binding_and_factory_values_parse() {
        let settings = parse_url(
            "jdbc:postgresql://h/db?channelBinding=require&sslfactory=org.postgresql.ssl.NonValidatingFactory",
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(settings.channel_binding, ChannelBinding::Require);
        assert_eq!(settings.ssl_factory, SslFactory::NonValidating);
        assert_eq!(settings.scram_max_iterations, 100_000);
        assert_eq!(settings.ssl_response_timeout, Some(Duration::from_secs(5)));
        let error = parse_url(
            "jdbc:postgresql://h/db?channelBinding=maybe",
            &HashMap::new(),
        )
        .unwrap_err();
        assert_eq!(error.message(), "Invalid channelBinding value: maybe");
    }
}
