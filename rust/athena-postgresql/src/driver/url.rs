use std::collections::HashMap;
use std::time::Duration;

use athena_jdbc::sql::SqlException;

use super::error::{psql, CONNECTION_UNABLE_TO_CONNECT, INVALID_PARAMETER_VALUE};

const URL_PREFIX: &str = "jdbc:postgresql:";
const DEFAULT_PORT: u16 = 5432;
const DEFAULT_CONNECT_TIMEOUT_SECONDS: u64 = 10;
const DEFAULT_APPLICATION_NAME: &str = "PostgreSQL JDBC Driver";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SslMode {
    Disable,
    Allow,
    Prefer,
    Require,
    VerifyCa,
    VerifyFull,
}

impl SslMode {
    fn parse(value: &str) -> Result<SslMode, SqlException> {
        match value {
            "disable" => Ok(SslMode::Disable),
            "allow" => Ok(SslMode::Allow),
            "prefer" => Ok(SslMode::Prefer),
            "require" => Ok(SslMode::Require),
            "verify-ca" => Ok(SslMode::VerifyCa),
            "verify-full" => Ok(SslMode::VerifyFull),
            other => Err(psql(
                format!("Invalid sslmode value: {other}"),
                CONNECTION_UNABLE_TO_CONNECT,
            )),
        }
    }

    pub(crate) fn requires_ssl(self) -> bool {
        matches!(
            self,
            SslMode::Require | SslMode::VerifyCa | SslMode::VerifyFull
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HostSpec {
    pub(crate) host: String,
    pub(crate) port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SslVerification {
    None,
    Chain,
    Full,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConnectSettings {
    pub(crate) hosts: Vec<HostSpec>,
    pub(crate) database: String,
    pub(crate) user: Option<String>,
    pub(crate) password: Option<String>,
    pub(crate) ssl_mode: SslMode,
    pub(crate) ssl_verification: SslVerification,
    pub(crate) ssl_root_cert: Option<String>,
    pub(crate) connect_timeout: Option<Duration>,
    pub(crate) socket_timeout: Option<Duration>,
    pub(crate) tcp_keep_alive: bool,
    pub(crate) application_name: String,
    pub(crate) current_schema: Option<String>,
    pub(crate) options: Option<String>,
    pub(crate) string_type_unspecified: bool,
    pub(crate) log_server_error_detail: bool,
    pub(crate) time_zone: String,
}

fn bad_url() -> SqlException {
    psql(
        "No suitable driver found for the given URL",
        CONNECTION_UNABLE_TO_CONNECT,
    )
}

fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn url_decode(text: &str) -> Result<String, SqlException> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' => {
                let decoded = bytes
                    .get(i + 1)
                    .and_then(|h| hex_value(*h))
                    .zip(bytes.get(i + 2).and_then(|l| hex_value(*l)))
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

fn parse_host(spec: &str) -> Result<HostSpec, SqlException> {
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
) -> Result<ConnectSettings, SqlException> {
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

fn seconds(properties: &HashMap<String, String>, key: &str, default: u64) -> Option<Duration> {
    let value = properties
        .get(key)
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(default);
    (value > 0).then(|| Duration::from_secs(value))
}

fn flag(properties: &HashMap<String, String>, key: &str, default: bool) -> bool {
    properties
        .get(key)
        .map_or(default, |v| v.eq_ignore_ascii_case("true"))
}

fn default_time_zone() -> String {
    std::env::var("TZ")
        .ok()
        .map(|tz| tz.trim_start_matches(':').to_string())
        .filter(|tz| !tz.is_empty())
        .unwrap_or_else(|| "UTC".to_string())
}

fn settings_from(
    hosts: Vec<HostSpec>,
    database: String,
    properties: &HashMap<String, String>,
) -> Result<ConnectSettings, SqlException> {
    let ssl_factory_non_validating = properties
        .get("sslfactory")
        .is_some_and(|f| f.ends_with("NonValidatingFactory"));
    let ssl_mode = match properties.get("sslmode") {
        Some(mode) => SslMode::parse(mode)?,
        None if flag(properties, "ssl", false) => SslMode::VerifyFull,
        None => SslMode::Prefer,
    };
    let ssl_verification = match ssl_mode {
        _ if ssl_factory_non_validating => SslVerification::None,
        SslMode::VerifyFull => SslVerification::Full,
        SslMode::VerifyCa => SslVerification::Chain,
        _ => SslVerification::None,
    };
    Ok(ConnectSettings {
        hosts,
        database,
        user: properties.get("user").cloned(),
        password: properties.get("password").cloned(),
        ssl_mode,
        ssl_verification,
        ssl_root_cert: properties.get("sslrootcert").cloned(),
        connect_timeout: seconds(
            properties,
            "connectTimeout",
            DEFAULT_CONNECT_TIMEOUT_SECONDS,
        ),
        socket_timeout: seconds(properties, "socketTimeout", 0),
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
        assert_eq!(settings.ssl_verification, SslVerification::None);
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
