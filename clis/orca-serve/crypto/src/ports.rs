use crate::boxes::KeyPair;
use crate::entropy::{random_array, Entropy};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError(pub String);

#[derive(Clone, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub token: String,
}

impl std::fmt::Debug for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Device").field("id", &self.id).finish_non_exhaustive()
    }
}

/// Where the host's long-lived x25519 identity lives: the phone pins its public half at pairing.
pub trait KeyPairStore {
    fn load(&self) -> Result<Option<KeyPair>, StoreError>;
    fn save(&self, pair: &KeyPair) -> Result<(), StoreError>;
}

/// Paired phones by bearer token. `find_by_token` is a lookup only: the caller still compares the token in constant time.
pub trait DeviceRegistry {
    fn find_by_token(&self, token: &str) -> Option<Device>;
    fn register(&self, device: Device) -> Result<(), StoreError>;
    fn revoke(&self, device_id: &str) -> Result<bool, StoreError>;
}

pub fn load_or_create_keypair(store: &dyn KeyPairStore, entropy: &dyn Entropy) -> Result<KeyPair, StoreError> {
    if let Some(pair) = store.load()? {
        return Ok(pair);
    }
    let pair = KeyPair::generate(entropy);
    store.save(&pair)?;
    Ok(pair)
}

/// A device id (random UUID, version 4) and a 24-byte hex token, the shapes the reference mints.
pub fn mint_device(entropy: &dyn Entropy) -> Device {
    let mut id: [u8; 16] = random_array(entropy);
    id[6] = (id[6] & 0x0f) | 0x40;
    id[8] = (id[8] & 0x3f) | 0x80;
    let h = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
    let id = format!("{}-{}-{}-{}-{}", h(&id[..4]), h(&id[4..6]), h(&id[6..8]), h(&id[8..10]), h(&id[10..]));
    let token: [u8; 24] = random_array(entropy);
    Device { id, token: h(&token) }
}
