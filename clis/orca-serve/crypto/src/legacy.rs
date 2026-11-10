//! The legacy framing: every frame is `nonce(24) || box`, text frames base64-encoded, one shared key per socket.
use crate::b64;
use crate::boxes::{secretbox_open, secretbox_seal, shared_key, KEY_LEN, NONCE_LEN};
use crate::entropy::{random_array, Entropy};

pub const READY_TEXT: &str = r#"{"type":"e2ee_ready"}"#;

pub struct LegacyChannel {
    key: [u8; KEY_LEN],
}

impl LegacyChannel {
    /// None when the client key is not a usable x25519 public key.
    pub fn new(server_secret: &[u8; KEY_LEN], client_public_b64: &str) -> Option<Self> {
        let client_public = b64::decode_canonical_exact::<KEY_LEN>(client_public_b64)?;
        Some(Self { key: shared_key(server_secret, &client_public)? })
    }

    pub fn seal_bytes(&self, entropy: &dyn Entropy, plaintext: &[u8]) -> Vec<u8> {
        let nonce: [u8; NONCE_LEN] = random_array(entropy);
        let mut frame = nonce.to_vec();
        frame.extend(secretbox_seal(&self.key, &nonce, plaintext));
        frame
    }

    pub fn open_bytes(&self, frame: &[u8]) -> Option<Vec<u8>> {
        let (nonce, boxed) = frame.split_at_checked(NONCE_LEN)?;
        secretbox_open(&self.key, nonce.try_into().ok()?, boxed)
    }

    pub fn seal_text(&self, entropy: &dyn Entropy, plaintext: &str) -> String {
        b64::encode(&self.seal_bytes(entropy, plaintext.as_bytes()))
    }

    pub fn open_text(&self, frame_b64: &str) -> Option<String> {
        String::from_utf8(self.open_bytes(&b64::decode_canonical(frame_b64)?)?).ok()
    }
}
