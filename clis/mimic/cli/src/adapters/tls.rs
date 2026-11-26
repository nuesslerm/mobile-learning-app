//! The disposable certificate authority of an explicit proxy. Its key exists only in this process: what is written is the CA's
//! certificate, for the test environment to trust, never a key. A leaf is made per approved host; the real server's certificate
//! is validated against the public roots, and against one more CA when the caller names it.
use rcgen::{BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair, KeyUsagePurpose};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, ServerConfig, ServerConnection, StreamOwned};
use std::collections::HashMap;
use std::net::TcpStream;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub type ServerSide = StreamOwned<ServerConnection, TcpStream>;
pub type ClientSide = StreamOwned<ClientConnection, TcpStream>;

const HTTP1: &[u8] = b"http/1.1";

fn said(e: impl std::fmt::Display) -> String {
    e.to_string()
}

pub struct Authority {
    issuer: Issuer<'static, KeyPair>,
    ca_pem: String,
    leaves: Mutex<HashMap<String, Arc<ServerConfig>>>,
}

impl Authority {
    pub fn new() -> Result<Authority, String> {
        let key = KeyPair::generate().map_err(said)?;
        let mut ca = CertificateParams::new(Vec::<String>::new()).map_err(said)?;
        ca.distinguished_name.push(DnType::CommonName, "wf mimic proxy test CA (disposable)");
        ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca.key_usages = vec![KeyUsagePurpose::KeyCertSign];
        let ca_pem = ca.self_signed(&key).map_err(said)?.pem();
        Ok(Authority { issuer: Issuer::new(ca, key), ca_pem, leaves: Mutex::default() })
    }

    /// The CA's certificate, for a trust store. There is no key to write.
    pub fn certificate_pem(&self) -> &str {
        &self.ca_pem
    }

    fn leaf(&self, host: &str) -> Result<Arc<ServerConfig>, String> {
        let key = KeyPair::generate().map_err(said)?;
        let mut leaf = CertificateParams::new(vec![host.to_string()]).map_err(said)?;
        leaf.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let certificate = leaf.signed_by(&key, &self.issuer).map_err(said)?;
        let chain = vec![CertificateDer::from_pem_slice(certificate.pem().as_bytes()).map_err(said)?];
        let key = PrivateKeyDer::from_pem_slice(key.serialize_pem().as_bytes()).map_err(said)?;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut config = ServerConfig::builder_with_provider(provider).with_safe_default_protocol_versions().map_err(said)?.with_no_client_auth().with_single_cert(chain, key).map_err(said)?;
        config.alpn_protocols = vec![HTTP1.to_vec()];
        Ok(Arc::new(config))
    }

    /// The client's side of an intercepted connection, as the approved host.
    pub fn accept(&self, host: &str, client: TcpStream) -> Result<ServerSide, String> {
        let config = {
            let mut leaves = self.leaves.lock().unwrap_or_else(|e| e.into_inner());
            match leaves.get(host) {
                Some(have) => have.clone(),
                None => {
                    let made = self.leaf(host)?;
                    leaves.insert(host.to_string(), made.clone());
                    made
                }
            }
        };
        Ok(StreamOwned::new(ServerConnection::new(config).map_err(said)?, client))
    }
}

/// How the proxy reaches a real server: the public roots, plus the CA of a fixture server when one is named.
pub fn upstream_config(extra_ca: Option<&Path>) -> Result<Arc<ClientConfig>, String> {
    let mut roots = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if let Some(path) = extra_ca {
        let pem = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        for certificate in CertificateDer::pem_slice_iter(&pem) {
            roots.add(certificate.map_err(|e| format!("{}: {e}", path.display()))?).map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = ClientConfig::builder_with_provider(provider).with_safe_default_protocol_versions().map_err(said)?.with_root_certificates(roots).with_no_client_auth();
    config.alpn_protocols = vec![HTTP1.to_vec()];
    Ok(Arc::new(config))
}

/// The server's side of an intercepted connection. The certificate is checked when the first bytes cross.
pub fn connect(config: &Arc<ClientConfig>, host: &str, server: TcpStream) -> Result<ClientSide, String> {
    let name = ServerName::try_from(host.to_string()).map_err(said)?;
    Ok(StreamOwned::new(ClientConnection::new(config.clone(), name).map_err(said)?, server))
}
