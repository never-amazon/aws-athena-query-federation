use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use hmac::{Hmac, KeyInit, Mac};
use md5::{Digest as _, Md5};
use sha2::Sha256;

use athena_jdbc::sql::SqlException;

use super::error::{protocol_violation, psql, INVALID_PASSWORD};

pub(crate) const SCRAM_SHA_256: &str = "SCRAM-SHA-256";
const GS2_HEADER: &str = "n,,";
const NONCE_BYTES: usize = 18;

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .flat_map(|b| {
            [
                char::from(HEX_DIGITS[usize::from(b >> 4)]),
                char::from(HEX_DIGITS[usize::from(b & 0x0f)]),
            ]
        })
        .collect()
}

pub(crate) fn md5_password(user: &str, password: &str, salt: &[u8]) -> String {
    let mut inner = Md5::new();
    inner.update(password.as_bytes());
    inner.update(user.as_bytes());
    let inner = hex(&inner.finalize());
    let mut outer = Md5::new();
    outer.update(inner.as_bytes());
    outer.update(salt);
    format!("md5{}", hex(&outer.finalize()))
}

fn hmac(key: &[u8], data: &[u8]) -> Result<Vec<u8>, SqlException> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key)
        .map_err(|_| protocol_violation("Invalid SCRAM key length"))?;
    mac.update(data);
    Ok(mac.finalize().into_bytes().to_vec())
}

fn salted_password(password: &[u8], salt: &[u8], iterations: u32) -> Result<Vec<u8>, SqlException> {
    let mut first = salt.to_vec();
    first.extend_from_slice(&1u32.to_be_bytes());
    let mut u = hmac(password, &first)?;
    let mut result = u.clone();
    for _ in 1..iterations {
        u = hmac(password, &u)?;
        result.iter_mut().zip(&u).for_each(|(r, b)| *r ^= b);
    }
    Ok(result)
}

fn scram_error(message: &str) -> SqlException {
    protocol_violation(format!("Invalid server-first-message: {message}"))
}

pub(crate) struct ScramClient {
    password: String,
    client_nonce: String,
    client_first_bare: String,
    auth_message: Option<String>,
    server_signature: Option<Vec<u8>>,
}

impl ScramClient {
    pub(crate) fn new(password: &str) -> Result<ScramClient, SqlException> {
        let mut nonce = [0u8; NONCE_BYTES];
        getrandom::fill(&mut nonce)
            .map_err(|e| protocol_violation(format!("Unable to generate a SCRAM nonce: {e}")))?;
        let client_nonce = STANDARD.encode(nonce);
        Ok(ScramClient {
            password: password.to_string(),
            client_first_bare: format!("n=*,r={client_nonce}"),
            client_nonce,
            auth_message: None,
            server_signature: None,
        })
    }

    pub(crate) fn client_first(&self) -> String {
        format!("{GS2_HEADER}{}", self.client_first_bare)
    }

    pub(crate) fn client_final(&mut self, server_first: &[u8]) -> Result<String, SqlException> {
        let server_first =
            std::str::from_utf8(server_first).map_err(|_| scram_error("not UTF-8"))?;
        let mut nonce = None;
        let mut salt = None;
        let mut iterations = None;
        for part in server_first.split(',') {
            if let Some(v) = part.strip_prefix("r=") {
                nonce = Some(v);
            } else if let Some(v) = part.strip_prefix("s=") {
                salt = Some(v);
            } else if let Some(v) = part.strip_prefix("i=") {
                iterations = Some(v);
            }
        }
        let nonce = nonce.ok_or_else(|| scram_error("missing nonce"))?;
        if !nonce.starts_with(&self.client_nonce) {
            return Err(scram_error(
                "server nonce does not start with the client nonce",
            ));
        }
        let salt = STANDARD
            .decode(salt.ok_or_else(|| scram_error("missing salt"))?)
            .map_err(|_| scram_error("bad salt"))?;
        let iterations: u32 = iterations
            .ok_or_else(|| scram_error("missing iteration count"))?
            .parse()
            .map_err(|_| scram_error("bad iteration count"))?;
        let salted = salted_password(self.password.as_bytes(), &salt, iterations)?;
        let client_key = hmac(&salted, b"Client Key")?;
        let stored_key = Sha256::digest(&client_key);
        let channel_binding = STANDARD.encode(GS2_HEADER);
        let without_proof = format!("c={channel_binding},r={nonce}");
        let auth_message = format!("{},{server_first},{without_proof}", self.client_first_bare);
        let client_signature = hmac(&stored_key, auth_message.as_bytes())?;
        let proof: Vec<u8> = client_key
            .iter()
            .zip(&client_signature)
            .map(|(a, b)| a ^ b)
            .collect();
        let server_key = hmac(&salted, b"Server Key")?;
        self.server_signature = Some(hmac(&server_key, auth_message.as_bytes())?);
        self.auth_message = Some(auth_message);
        Ok(format!("{without_proof},p={}", STANDARD.encode(proof)))
    }

    pub(crate) fn verify_server_final(&self, server_final: &[u8]) -> Result<(), SqlException> {
        let text = std::str::from_utf8(server_final).unwrap_or_default();
        if let Some(error) = text.strip_prefix("e=") {
            return Err(psql(
                format!("SCRAM authentication failed: {error}"),
                INVALID_PASSWORD,
            ));
        }
        let signature = text
            .strip_prefix("v=")
            .and_then(|v| STANDARD.decode(v).ok())
            .ok_or_else(|| protocol_violation("Invalid server-final-message"))?;
        match &self.server_signature {
            Some(expected) if *expected == signature && self.auth_message.is_some() => Ok(()),
            _ => Err(protocol_violation("Invalid server SCRAM signature")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_password_matches_libpq() {
        assert_eq!(
            md5_password("postgres", "secret", &[1, 2, 3, 4]),
            "md5bb41a296aab6baccb36ff243a562abff"
        );
    }

    #[test]
    fn scram_follows_rfc_7677_example() {
        let mut client = ScramClient {
            password: "pencil".to_string(),
            client_nonce: "rOprNGfwEbeRWgbNEkqO".to_string(),
            client_first_bare: "n=user,r=rOprNGfwEbeRWgbNEkqO".to_string(),
            auth_message: None,
            server_signature: None,
        };
        let client_final = client
            .client_final(b"r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0,s=W22ZaJ0SNY7soEsUEjb6gQ==,i=4096")
            .unwrap();
        assert_eq!(
            client_final,
            "c=biws,r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0,p=dHzbZapWIk4jUhN+Ute9ytag9zjfMHgsqmmiz7AndVQ="
        );
        client
            .verify_server_final(b"v=6rriTRBi23WpRR/wtup+mMhUZUn/dB5nLTJRsjl95G4=")
            .unwrap();
    }
}
