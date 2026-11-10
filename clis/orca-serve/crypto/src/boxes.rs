//! NaCl `crypto_box` pieces: x25519 keys and the `box_beforenm` shared key (x25519 then HSalsa20).
use crate::entropy::{random_array, Entropy};
use salsa20::cipher::consts::U10;
use salsa20::cipher::generic_array::GenericArray;
use x25519_dalek::{x25519, X25519_BASEPOINT_BYTES};

pub const KEY_LEN: usize = 32;

#[derive(Clone, PartialEq, Eq)]
pub struct KeyPair {
    pub public: [u8; KEY_LEN],
    pub secret: [u8; KEY_LEN],
}

impl std::fmt::Debug for KeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyPair").field("public", &self.public).finish_non_exhaustive()
    }
}

impl KeyPair {
    pub fn from_secret(secret: [u8; KEY_LEN]) -> Self {
        Self { public: x25519(secret, X25519_BASEPOINT_BYTES), secret }
    }

    pub fn generate(entropy: &dyn Entropy) -> Self {
        Self::from_secret(random_array(entropy))
    }
}

/// `nacl.box.before`. None for a peer key of small order, whose shared secret would be all zero (the reference accepts it; refusing is stricter).
pub fn shared_key(our_secret: &[u8; KEY_LEN], peer_public: &[u8; KEY_LEN]) -> Option<[u8; KEY_LEN]> {
    let dh = x25519(*our_secret, *peer_public);
    if dh.iter().all(|b| *b == 0) {
        return None;
    }
    let key = salsa20::hsalsa::<U10>(GenericArray::from_slice(&dh), &GenericArray::default());
    Some(key.into())
}

pub const NONCE_LEN: usize = 24;
pub const TAG_LEN: usize = 16;

pub fn secretbox_seal(key: &[u8; KEY_LEN], nonce: &[u8; NONCE_LEN], message: &[u8]) -> Vec<u8> {
    use crypto_secretbox::aead::{Aead, KeyInit};
    crypto_secretbox::XSalsa20Poly1305::new(GenericArray::from_slice(key))
        .encrypt(GenericArray::from_slice(nonce), message)
        .expect("sealing into a Vec cannot fail")
}

/// None for any box that fails authentication, including one shorter than its tag.
pub fn secretbox_open(key: &[u8; KEY_LEN], nonce: &[u8; NONCE_LEN], boxed: &[u8]) -> Option<Vec<u8>> {
    use crypto_secretbox::aead::{Aead, KeyInit};
    if boxed.len() < TAG_LEN {
        return None;
    }
    crypto_secretbox::XSalsa20Poly1305::new(GenericArray::from_slice(key))
        .decrypt(GenericArray::from_slice(nonce), boxed)
        .ok()
}
