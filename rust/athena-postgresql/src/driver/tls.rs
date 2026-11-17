use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::WebPkiServerVerifier;
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::{
    CertificateError, ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme,
};
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, ServerName, UnixTime};

use athena_jdbc::sql::SqlException;

use super::error::{psql, CONNECTION_UNABLE_TO_CONNECT};
use super::url::SslVerification;

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
    psql(message, CONNECTION_UNABLE_TO_CONNECT)
}

fn root_store(root_cert: Option<&str>) -> Result<RootCertStore, SqlException> {
    let mut roots = RootCertStore::empty();
    match root_cert {
        Some(path) => {
            let certs = CertificateDer::pem_file_iter(path)
                .map_err(|e| {
                    ssl_error(format!(
                        "Could not open SSL root certificate file {path}. {e}"
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| {
                    ssl_error(format!(
                        "Could not read SSL root certificate file {path}. {e}"
                    ))
                })?;
            for cert in certs {
                roots.add(cert).map_err(|e| {
                    ssl_error(format!(
                        "Could not read SSL root certificate file {path}. {e}"
                    ))
                })?;
            }
        }
        None => roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned()),
    }
    Ok(roots)
}

pub(crate) fn client_config(
    verification: &SslVerification,
    root_cert: Option<&str>,
) -> Result<Arc<ClientConfig>, SqlException> {
    let provider = provider();
    let builder = ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .map_err(|e| ssl_error(format!("SSL error: {e}")))?;
    let config = match verification {
        SslVerification::None => builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAnyCertificate { provider }))
            .with_no_client_auth(),
        SslVerification::Full => {
            let verifier = WebPkiServerVerifier::builder_with_provider(
                Arc::new(root_store(root_cert)?),
                provider,
            )
            .build()
            .map_err(|e| ssl_error(format!("SSL error: {e}")))?;
            builder.with_webpki_verifier(verifier).with_no_client_auth()
        }
        SslVerification::Chain => {
            let inner = WebPkiServerVerifier::builder_with_provider(
                Arc::new(root_store(root_cert)?),
                provider,
            )
            .build()
            .map_err(|e| ssl_error(format!("SSL error: {e}")))?;
            builder
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(ChainOnly { inner }))
                .with_no_client_auth()
        }
    };
    Ok(Arc::new(config))
}
