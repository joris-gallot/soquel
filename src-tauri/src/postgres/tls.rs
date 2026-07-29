use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::ring;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
use rustls_platform_verifier::BuilderVerifierExt;
use tokio_postgres_rustls::MakeRustlsConnect;

use crate::error::Error;
use crate::profiles::SslMode;

// Verification level is the verifier's job; the wire protocol only knows
// whether to attempt TLS.
pub fn config_ssl_mode(mode: SslMode) -> tokio_postgres::config::SslMode {
  match mode {
    SslMode::Disable => tokio_postgres::config::SslMode::Disable,
    SslMode::Prefer => tokio_postgres::config::SslMode::Prefer,
    SslMode::Require | SslMode::VerifyFull => tokio_postgres::config::SslMode::Require,
  }
}

pub fn connector(mode: SslMode) -> Result<MakeRustlsConnect, Error> {
  let config = match mode {
    // libpq parity: everything below verify-full encrypts without verifying.
    SslMode::Disable | SslMode::Prefer | SslMode::Require => ClientConfig::builder()
      .dangerous()
      .with_custom_certificate_verifier(Arc::new(AcceptAllVerifier))
      .with_no_client_auth(),
    SslMode::VerifyFull => ClientConfig::builder()
      .with_platform_verifier()
      .map_err(|err| Error::Database {
        message: format!("tls setup: {err}"),
      })?
      .with_no_client_auth(),
  };
  Ok(MakeRustlsConnect::new(config))
}

#[derive(Debug)]
struct AcceptAllVerifier;

impl ServerCertVerifier for AcceptAllVerifier {
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
    _message: &[u8],
    _cert: &CertificateDer<'_>,
    _dss: &DigitallySignedStruct,
  ) -> Result<HandshakeSignatureValid, rustls::Error> {
    Ok(HandshakeSignatureValid::assertion())
  }

  fn verify_tls13_signature(
    &self,
    _message: &[u8],
    _cert: &CertificateDer<'_>,
    _dss: &DigitallySignedStruct,
  ) -> Result<HandshakeSignatureValid, rustls::Error> {
    Ok(HandshakeSignatureValid::assertion())
  }

  fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
    ring::default_provider()
      .signature_verification_algorithms
      .supported_schemes()
  }
}
