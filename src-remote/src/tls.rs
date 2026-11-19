use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, Error, RootCertStore, ServerConfig, SignatureScheme};

use crate::crypto::{b64, sha256, unb64};
use crate::store::TlsMaterial;

pub fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

pub fn generate() -> TlsMaterial {
    let ck = rcgen::generate_simple_self_signed(vec!["shockingvrc.local".to_string()])
        .expect("self-signed certificate generation");
    TlsMaterial {
        cert_der: b64(ck.cert.der()),
        key_der: b64(&ck.signing_key.serialize_der()),
    }
}

pub fn pin_of(cert_der: &[u8]) -> String {
    b64(&sha256(&[cert_der]))
}

pub fn material_pin(m: &TlsMaterial) -> Option<String> {
    unb64(&m.cert_der).map(|d| pin_of(&d))
}

pub fn server_config(m: &TlsMaterial) -> Result<Arc<ServerConfig>, String> {
    let cert = unb64(&m.cert_der).ok_or("bad certificate")?;
    let key = unb64(&m.key_der).ok_or("bad key")?;
    let cfg = ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(cert)],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key)),
        )
        .map_err(|e| e.to_string())?;
    Ok(Arc::new(cfg))
}

#[derive(Debug)]
struct PinVerifier {
    pin: Vec<u8>,
    algs: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for PinVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        let matches = std::iter::once(end_entity)
            .chain(intermediates)
            .any(|c| crate::crypto::ct_eq(&sha256(&[c.as_ref()]), &self.pin));
        if matches {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(Error::General("certificate does not match the pinned fingerprint".into()))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algs)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algs)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algs.supported_schemes()
    }
}

pub fn pinned_client(pin: &str) -> Result<Arc<ClientConfig>, String> {
    let pin = unb64(pin).filter(|p| p.len() == 32).ok_or("bad certificate pin")?;
    let provider = provider();
    let algs = provider.signature_verification_algorithms;
    let cfg = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinVerifier { pin, algs }))
        .with_no_client_auth();
    Ok(Arc::new(cfg))
}

pub fn webpki_client() -> Arc<ClientConfig> {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let cfg = ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .expect("ring supports the default protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    Arc::new(cfg)
}

pub fn client_for(pin: Option<&str>) -> Result<Arc<ClientConfig>, String> {
    match pin.filter(|p| !p.is_empty()) {
        Some(p) => pinned_client(p),
        None => Ok(webpki_client()),
    }
}
