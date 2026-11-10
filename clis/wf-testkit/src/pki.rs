//! Keys, certificates and digests for the fixtures, from crates: no `openssl` process.

use ed25519_dalek::pkcs8::spki::der::pem::LineEnding;
use ed25519_dalek::pkcs8::{EncodePrivateKey, EncodePublicKey};
use ed25519_dalek::SigningKey;
use hmac::{Hmac, Mac};
#[cfg(feature = "tls")]
use rcgen::{BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose};
use sha2::{Digest, Sha256};

/// A throwaway CA and a server certificate for 127.0.0.1 signed by it (PEM texts). A certificate that is both its own CA and
/// the server's is refused by rustls and webpki.
#[cfg(feature = "tls")]
pub struct TestPki {
    pub ca_pem: String,
    pub cert_pem: String,
    pub key_pem: String,
}

#[cfg(feature = "tls")]
pub fn ca_and_localhost_leaf() -> TestPki {
    let ca_key = KeyPair::generate().expect("a CA key");
    let mut ca = CertificateParams::new(Vec::<String>::new()).expect("CA parameters");
    ca.distinguished_name.push(DnType::CommonName, "wf-test-ca");
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca.key_usages = vec![KeyUsagePurpose::KeyCertSign];
    let ca_cert = ca.self_signed(&ca_key).expect("a CA certificate");
    let leaf_key = KeyPair::generate().expect("a leaf key");
    let mut leaf = CertificateParams::new(vec!["127.0.0.1".to_string()]).expect("leaf parameters");
    leaf.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let issuer = rcgen::Issuer::new(ca, ca_key);
    let leaf_cert = leaf.signed_by(&leaf_key, &issuer).expect("a leaf certificate");
    TestPki { ca_pem: ca_cert.pem(), cert_pem: leaf_cert.pem(), key_pem: leaf_key.serialize_pem() }
}

/// A fresh Ed25519 key: the PKCS#8 private PEM and the public PEM.
pub fn ed25519_pem_pair() -> (String, String) {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).expect("system randomness");
    let key = SigningKey::from_bytes(&seed);
    let private = key.to_pkcs8_pem(LineEnding::LF).expect("a private PEM").to_string();
    let public = key.verifying_key().to_public_key_pem(LineEnding::LF).expect("a public PEM");
    (private, public)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

pub fn hmac_sha256_hex(key: &str, data: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes()).expect("an HMAC key");
    mac.update(data);
    hex(&mac.finalize().into_bytes())
}

/// Whether `signature` is a valid Ed25519 signature of `message` under the public PEM.
pub fn ed25519_verifies(public_pem: &str, message: &[u8], signature: &[u8]) -> bool {
    use ed25519_dalek::pkcs8::DecodePublicKey;
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    let Ok(key) = VerifyingKey::from_public_key_pem(public_pem) else { return false };
    let Ok(signature) = Signature::from_slice(signature) else { return false };
    key.verify(message, &signature).is_ok()
}
