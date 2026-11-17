use std::path::PathBuf;
use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::{verify_server_name, WebPkiServerVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::server::ParsedCertificate;
use rustls::{
    CertificateError, ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme,
};
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, ServerName, UnixTime};
use sha2::{Digest, Sha224, Sha256, Sha384, Sha512};

use athena_jdbc::sql::SqlException;

use super::error::{psql, CONNECTION_FAILURE};
use super::url::{ConnectSettings, SslFactory, SslNegotiation};

const HOSTNAME_VERIFIER: &str = "PgjdbcHostnameVerifier";

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

#[derive(Debug)]
struct AcceptAnyCertificate {
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for AcceptAnyCertificate {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[derive(Debug)]
struct ChainOnly {
    inner: Arc<WebPkiServerVerifier>,
}

impl ServerCertVerifier for ChainOnly {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        match self.inner.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        ) {
            Err(rustls::Error::InvalidCertificate(
                CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. },
            )) => Ok(ServerCertVerified::assertion()),
            other => other,
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

fn ssl_error(message: impl Into<String>) -> SqlException {
    psql(message, CONNECTION_FAILURE)
}

pub(crate) fn default_root_cert() -> PathBuf {
    let home = std::env::var_os("HOME").unwrap_or_else(|| "?".into());
    PathBuf::from(home).join(".postgresql").join("root.crt")
}

async fn root_file_store(path: &str) -> Result<RootCertStore, SqlException> {
    let bytes = tokio::fs::read(path).await.map_err(|e| {
        ssl_error(format!("Could not open SSL root certificate file {path}.")).with_cause(e)
    })?;
    let load_failed = || {
        ssl_error(format!(
            "Loading the SSL root certificate {path} into a TrustManager failed."
        ))
    };
    let certs = CertificateDer::pem_slice_iter(&bytes)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| load_failed().with_cause(e))?;
    let mut roots = RootCertStore::empty();
    for cert in certs {
        roots.add(cert).map_err(|e| load_failed().with_cause(e))?;
    }
    if roots.is_empty() {
        return Err(load_failed());
    }
    Ok(roots)
}

fn chain_verifier(
    roots: RootCertStore,
    provider: Arc<CryptoProvider>,
) -> Result<Arc<dyn ServerCertVerifier>, SqlException> {
    let inner = WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider)
        .build()
        .map_err(|e| ssl_error("Could not initialize SSL context.").with_cause(e))?;
    Ok(Arc::new(ChainOnly { inner }))
}

async fn verifier(settings: &ConnectSettings) -> Result<Arc<dyn ServerCertVerifier>, SqlException> {
    let provider = provider();
    match &settings.ssl_factory {
        SslFactory::Unsupported(name) => Err(ssl_error(format!(
            "The SSLSocketFactory class provided {name} could not be instantiated."
        ))),
        SslFactory::NonValidating => Ok(Arc::new(AcceptAnyCertificate { provider })),
        SslFactory::DefaultJava => {
            let roots = RootCertStore {
                roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
            };
            chain_verifier(roots, provider)
        }
        SslFactory::LibPq if !settings.ssl_mode.verify_certificate() => {
            Ok(Arc::new(AcceptAnyCertificate { provider }))
        }
        SslFactory::LibPq => {
            let path = settings
                .ssl_root_cert
                .clone()
                .unwrap_or_else(|| default_root_cert().to_string_lossy().into_owned());
            chain_verifier(root_file_store(&path).await?, provider)
        }
    }
}

pub(crate) async fn client_config(
    settings: &ConnectSettings,
) -> Result<Arc<ClientConfig>, SqlException> {
    let verifier = verifier(settings).await?;
    let mut config = ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .map_err(|e| ssl_error("Could not initialize SSL context.").with_cause(e))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    if settings.ssl_negotiation == SslNegotiation::Direct {
        config.alpn_protocols = vec![b"postgresql".to_vec()];
    }
    Ok(Arc::new(config))
}

pub(crate) fn server_name(host: &str) -> Result<ServerName<'static>, SqlException> {
    let bare = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);
    ServerName::try_from(bare.to_string()).map_err(|e| {
        ssl_error(format!(
            "The hostname {host} could not be verified by hostnameverifier {HOSTNAME_VERIFIER}."
        ))
        .with_cause(e)
    })
}

pub(crate) fn verify_peer_name(host: &str, end_entity: Option<&[u8]>) -> Result<(), SqlException> {
    let failed = || {
        ssl_error(format!(
            "The hostname {host} could not be verified by hostnameverifier {HOSTNAME_VERIFIER}."
        ))
    };
    let cert = CertificateDer::from(end_entity.ok_or_else(failed)?);
    let parsed = ParsedCertificate::try_from(&cert).map_err(|e| failed().with_cause(e))?;
    verify_server_name(&parsed, &server_name(host)?).map_err(|e| failed().with_cause(e))
}

struct Der<'a> {
    bytes: &'a [u8],
}

impl<'a> Der<'a> {
    fn element(&mut self) -> Option<(u8, &'a [u8])> {
        let (&tag, rest) = self.bytes.split_first()?;
        let (&first, rest) = rest.split_first()?;
        let (len, rest) = if first < 0x80 {
            (usize::from(first), rest)
        } else {
            let count = usize::from(first & 0x7f);
            if count == 0 || count > 4 || rest.len() < count {
                return None;
            }
            let len = rest[..count]
                .iter()
                .fold(0usize, |n, b| (n << 8) | usize::from(*b));
            (len, &rest[count..])
        };
        if rest.len() < len {
            return None;
        }
        let (content, after) = rest.split_at(len);
        self.bytes = after;
        Some((tag, content))
    }
}

const SEQUENCE: u8 = 0x30;
const OBJECT_IDENTIFIER: u8 = 0x06;

fn signature_algorithm(cert: &[u8]) -> Option<&[u8]> {
    let (tag, certificate) = Der { bytes: cert }.element()?;
    if tag != SEQUENCE {
        return None;
    }
    let mut fields = Der { bytes: certificate };
    fields.element()?;
    let (tag, algorithm) = fields.element()?;
    if tag != SEQUENCE {
        return None;
    }
    let (tag, oid) = Der { bytes: algorithm }.element()?;
    (tag == OBJECT_IDENTIFIER).then_some(oid)
}

const SHA224_OIDS: [&[u8]; 2] = [
    &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0e],
    &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x01],
];
const SHA384_OIDS: [&[u8]; 2] = [
    &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0c],
    &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x03],
];
const SHA512_OIDS: [&[u8]; 2] = [
    &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0d],
    &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x04],
];

pub(crate) fn tls_server_end_point(cert: &[u8]) -> Option<Vec<u8>> {
    let oid = signature_algorithm(cert)?;
    Some(if SHA224_OIDS.contains(&oid) {
        Sha224::digest(cert).to_vec()
    } else if SHA384_OIDS.contains(&oid) {
        Sha384::digest(cert).to_vec()
    } else if SHA512_OIDS.contains(&oid) {
        Sha512::digest(cert).to_vec()
    } else {
        Sha256::digest(cert).to_vec()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn certificate(oid: &[u8]) -> Vec<u8> {
        let algorithm = [
            &[OBJECT_IDENTIFIER, u8::try_from(oid.len()).unwrap()][..],
            oid,
        ]
        .concat();
        let algorithm = [
            &[SEQUENCE, u8::try_from(algorithm.len()).unwrap()][..],
            &algorithm,
        ]
        .concat();
        let body = [&[SEQUENCE, 0][..], &algorithm, &[0x03, 1, 0]].concat();
        [&[SEQUENCE, u8::try_from(body.len()).unwrap()][..], &body].concat()
    }

    #[test]
    fn channel_binding_hash_follows_the_signature_algorithm() {
        let sha256_rsa = certificate(&[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0b]);
        assert_eq!(tls_server_end_point(&sha256_rsa).unwrap().len(), 32);
        let sha1_rsa = certificate(&[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x05]);
        assert_eq!(tls_server_end_point(&sha1_rsa).unwrap().len(), 32);
        assert_eq!(
            tls_server_end_point(&certificate(SHA384_OIDS[1]))
                .unwrap()
                .len(),
            48
        );
        assert_eq!(
            tls_server_end_point(&certificate(SHA512_OIDS[0]))
                .unwrap()
                .len(),
            64
        );
        assert_eq!(
            tls_server_end_point(&certificate(SHA224_OIDS[0]))
                .unwrap()
                .len(),
            28
        );
        assert!(tls_server_end_point(&[0x30, 0x05, 1]).is_none());
    }

    #[tokio::test]
    async fn missing_default_root_file_fails_like_pgjdbc() {
        let error = root_file_store("/nonexistent/f2pg/root.crt")
            .await
            .unwrap_err();
        assert_eq!(
            error.message(),
            "Could not open SSL root certificate file /nonexistent/f2pg/root.crt."
        );
        assert_eq!(error.sql_state(), Some("08006"));
        assert!(default_root_cert().ends_with(".postgresql/root.crt"));
    }
}
