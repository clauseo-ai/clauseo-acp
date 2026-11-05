//! The TLS-inspecting proxy's certificates: a CA made at test time, never written to the repo,
//! and a leaf for each host a `CONNECT` names, signed by it.

use std::io;
use std::net::TcpStream;
use std::sync::Arc;

use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, DnType, IsCa, KeyPair, KeyUsagePurpose,
};
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};

/// A test CA and its key, in memory.
pub(super) struct TestCa {
    issuer: CertifiedIssuer<'static, KeyPair>,
}

impl std::fmt::Debug for TestCa {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TestCa")
    }
}

impl TestCa {
    /// A new CA, `Clauseo test CA`.
    pub(super) fn new() -> io::Result<TestCa> {
        let mut params = CertificateParams::new(Vec::<String>::new()).map_err(io::Error::other)?;
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params
            .distinguished_name
            .push(DnType::CommonName, "Clauseo test CA");
        params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
            KeyUsagePurpose::DigitalSignature,
        ];
        let key = KeyPair::generate().map_err(io::Error::other)?;
        let issuer = CertifiedIssuer::self_signed(params, key).map_err(io::Error::other)?;
        Ok(TestCa { issuer })
    }

    /// The CA's certificate as PEM.
    pub(super) fn pem(&self) -> String {
        self.issuer.pem()
    }

    /// Terminates TLS on `stream` as `host`, with a leaf made for it now.
    pub(super) fn accept(
        &self,
        host: &str,
        stream: TcpStream,
    ) -> io::Result<StreamOwned<ServerConnection, TcpStream>> {
        let params = CertificateParams::new(vec![host.to_owned()]).map_err(io::Error::other)?;
        let key = KeyPair::generate().map_err(io::Error::other)?;
        let leaf = params
            .signed_by(&key, &self.issuer)
            .map_err(io::Error::other)?;
        let private = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(io::Error::other)?
            .with_no_client_auth()
            .with_single_cert(vec![leaf.der().clone(), self.issuer.der().clone()], private)
            .map_err(io::Error::other)?;
        let conn = ServerConnection::new(Arc::new(config)).map_err(io::Error::other)?;
        Ok(StreamOwned::new(conn, stream))
    }
}
