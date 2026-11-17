use std::collections::HashMap;
use std::fmt;
use std::sync::LazyLock;

use athena_federation_sdk::exceptions::FederationSourceErrorCode;
use athena_federation_sdk::{FederationError, Result};
use base64::Engine;
use pkcs8::der::asn1::{AnyRef, OctetStringRef};
use pkcs8::der::pem::LineEnding;
use pkcs8::der::Decode;
use pkcs8::spki::AlgorithmIdentifierRef;
use pkcs8::{EncryptedPrivateKeyInfo, ObjectIdentifier, PrivateKeyInfo};
use regex::Regex;
use rsa::pkcs8::DecodePrivateKey;

use super::pbe::PbeScheme;
use crate::snowflake_constants::{is_blank, java_trim, SnowflakeConstants};
use crate::utils::SnowflakeAuthType;

const PEM_LINE_LENGTH: usize = 64;
const RSA_ENCRYPTION_OID: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.1");

static PEM_BEGIN_LABEL: LazyLock<Regex> = LazyLock::new(|| compile("-----BEGIN ([^-]+)-----"));
static PEM_BEGIN_ANY: LazyLock<Regex> = LazyLock::new(|| compile("-----BEGIN[^-]*-----"));
static PEM_END_ANY: LazyLock<Regex> = LazyLock::new(|| compile("-----END[^-]*-----"));
static JAVA_REGEX_WHITESPACE: LazyLock<Regex> = LazyLock::new(|| compile("[ \\t\\n\\x0B\\f\\r]"));

#[allow(clippy::expect_used, reason = "the patterns are fixed literals")]
fn compile(pattern: &str) -> Regex {
    Regex::new(pattern).expect("valid literal pattern")
}

#[derive(Clone, PartialEq, Eq)]
pub struct SnowflakePrivateKey {
    der: Vec<u8>,
}

impl SnowflakePrivateKey {
    pub fn der(&self) -> &[u8] {
        &self.der
    }

    pub fn to_pem(&self) -> Result<String> {
        pkcs8::der::pem::encode_string("PRIVATE KEY", LineEnding::LF, &self.der)
            .map_err(|e| FederationError::internal(format!("Cannot encode private key: {e}")))
    }
}

impl fmt::Debug for SnowflakePrivateKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SnowflakePrivateKey(<redacted>)")
    }
}

pub struct SnowflakeAuthUtils;

impl SnowflakeAuthUtils {
    pub fn determine_auth_type(credentials: &HashMap<String, String>) -> Result<SnowflakeAuthType> {
        if credentials.is_empty() {
            return Err(FederationError::illegal_argument(
                "Credentials cannot be null or empty",
            ));
        }
        let auth_type = if !is_blank(value(credentials, SnowflakeConstants::PEM_PRIVATE_KEY)) {
            tracing::debug!("Key-pair authentication detected");
            SnowflakeAuthType::SnowflakeJwt
        } else if !is_blank(value(credentials, SnowflakeConstants::AUTH_CODE)) {
            tracing::debug!("OAuth authentication detected");
            SnowflakeAuthType::Oauth
        } else {
            tracing::debug!("Password authentication detected");
            SnowflakeAuthType::Snowflake
        };
        Ok(auth_type)
    }

    pub fn create_private_key(
        private_key_string: &str,
        passphrase: Option<&str>,
    ) -> Result<SnowflakePrivateKey> {
        let pem = normalize_to_pem(private_key_string, passphrase);
        parse_pem_object(&pem, passphrase)
            .map(|der| SnowflakePrivateKey { der })
            .map_err(|message| {
                tracing::error!("Private key parsing failed: {message}");
                FederationError::connector(
                    FederationSourceErrorCode::InvalidInputException,
                    "Invalid private key format",
                )
            })
    }

    pub fn username(credentials: &HashMap<String, String>) -> Result<String> {
        [
            SnowflakeConstants::SF_USER,
            SnowflakeConstants::USERNAME,
            SnowflakeConstants::USERNAME_UPPERCASE,
        ]
        .iter()
        .filter_map(|key| value(credentials, key))
        .find(|candidate| !is_blank(Some(candidate)))
        .map(ToString::to_string)
        .ok_or_else(|| {
            FederationError::illegal_argument("Missing required parameter: username/sfUser")
        })
    }

    pub fn password(credentials: &HashMap<String, String>) -> Result<String> {
        [
            SnowflakeConstants::PASSWORD,
            SnowflakeConstants::PASSWORD_UPPERCASE,
        ]
        .iter()
        .filter_map(|key| value(credentials, key))
        .find(|candidate| !is_blank(Some(candidate)))
        .map(ToString::to_string)
        .ok_or_else(|| {
            FederationError::illegal_argument("password is required for password authentication")
        })
    }

    pub fn validate_credentials(
        credentials: &HashMap<String, String>,
        auth_type: SnowflakeAuthType,
    ) -> Result<()> {
        if credentials.is_empty() {
            return Err(FederationError::illegal_argument(
                "Credentials cannot be null or empty",
            ));
        }
        Self::username(credentials)?;
        match auth_type {
            SnowflakeAuthType::SnowflakeJwt => {
                if is_blank(value(credentials, SnowflakeConstants::PEM_PRIVATE_KEY)) {
                    return Err(FederationError::illegal_argument(
                        "pem_private_key is required for key-pair authentication",
                    ));
                }
            }
            SnowflakeAuthType::Oauth => {
                if is_blank(value(credentials, SnowflakeConstants::AUTH_CODE)) {
                    return Err(FederationError::illegal_argument(
                        "Auth code is required for OAuth authentication",
                    ));
                }
            }
            SnowflakeAuthType::Snowflake => {
                Self::password(credentials)?;
            }
        }
        Ok(())
    }
}

fn value<'a>(credentials: &'a HashMap<String, String>, key: &str) -> Option<&'a str> {
    credentials.get(key).map(String::as_str)
}

fn has_passphrase(passphrase: Option<&str>) -> bool {
    passphrase.is_some_and(|p| !java_trim(p).is_empty())
}

fn normalize_to_pem(private_key_string: &str, passphrase: Option<&str>) -> String {
    let key_type = if let Some(found) = PEM_BEGIN_LABEL.captures(private_key_string) {
        java_trim(&found[1]).to_string()
    } else if has_passphrase(passphrase) {
        "ENCRYPTED PRIVATE KEY".to_string()
    } else {
        "PRIVATE KEY".to_string()
    };
    let without_begin = PEM_BEGIN_ANY.replace_all(private_key_string, "");
    let without_end = PEM_END_ANY.replace_all(&without_begin, "");
    let body = JAVA_REGEX_WHITESPACE.replace_all(&without_end, "");
    let mut pem = format!("-----BEGIN {key_type}-----\n");
    let mut rest = body.as_ref();
    while !rest.is_empty() {
        let cut = rest
            .char_indices()
            .nth(PEM_LINE_LENGTH)
            .map_or(rest.len(), |(index, _)| index);
        pem.push_str(&rest[..cut]);
        pem.push('\n');
        rest = &rest[cut..];
    }
    pem = format!("{pem}-----END {key_type}-----\n");
    tracing::debug!("Normalized private key into well-formed PEM ({key_type})");
    pem
}

fn parse_pem_object(pem: &str, passphrase: Option<&str>) -> std::result::Result<Vec<u8>, String> {
    let (label, der) = read_pem(pem)?;
    match label.as_str() {
        "ENCRYPTED PRIVATE KEY" => {
            let (algorithm, ciphertext) = read_encrypted_private_key_info(&der)?;
            let Some(passphrase) = passphrase.filter(|p| !java_trim(p).is_empty()) else {
                return Err("Passphrase is required for encrypted private key".to_string());
            };
            let plaintext = match PbeScheme::from_oid(algorithm.oid) {
                Some(scheme) => scheme.decrypt(algorithm.parameters, passphrase, ciphertext)?,
                None => EncryptedPrivateKeyInfo::try_from(der.as_slice())
                    .map_err(|e| e.to_string())?
                    .decrypt(passphrase.as_bytes())
                    .map_err(|e| e.to_string())?
                    .as_bytes()
                    .to_vec(),
            };
            check_private_key_info(&plaintext)?;
            Ok(plaintext)
        }
        "PRIVATE KEY" => {
            check_private_key_info(&der)?;
            Ok(der)
        }
        other => Err(format!(
            "Unsupported private key format. Expected PKCS8EncryptedPrivateKeyInfo or PrivateKeyInfo, got: {other}"
        )),
    }
}

fn read_encrypted_private_key_info(
    der: &[u8],
) -> std::result::Result<(AlgorithmIdentifierRef<'_>, &[u8]), String> {
    AnyRef::from_der(der)
        .and_then(|any| {
            any.sequence(|reader| {
                let algorithm = AlgorithmIdentifierRef::decode(reader)?;
                let ciphertext = OctetStringRef::decode(reader)?;
                Ok((algorithm, ciphertext.as_bytes()))
            })
        })
        .map_err(|e| e.to_string())
}

fn read_pem(pem: &str) -> std::result::Result<(String, Vec<u8>), String> {
    let mut lines = pem.lines();
    let label = lines
        .next()
        .and_then(|line| line.strip_prefix("-----BEGIN "))
        .and_then(|line| line.strip_suffix("-----"))
        .ok_or_else(|| "missing PEM header".to_string())?
        .to_string();
    let body: String = lines
        .take_while(|line| !line.starts_with("-----END "))
        .collect();
    let der = base64::engine::general_purpose::STANDARD
        .decode(body)
        .map_err(|e| e.to_string())?;
    Ok((label, der))
}

fn check_private_key_info(der: &[u8]) -> std::result::Result<(), String> {
    let info = PrivateKeyInfo::try_from(der).map_err(|e| e.to_string())?;
    if info.algorithm.oid == RSA_ENCRYPTION_OID {
        rsa::RsaPrivateKey::from_pkcs8_der(der).map_err(|e| e.to_string())?;
    }
    Ok(())
}
