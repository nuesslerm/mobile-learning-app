//! In-memory doubles of every port, and a deterministic entropy source.
use crate::boxes::KeyPair;
use crate::entropy::Entropy;
use crate::ports::{Device, DeviceRegistry, KeyPairStore, StoreError};
use std::sync::Mutex;

/// Fills each request with the next bytes of a repeating pattern, so every nonce and key is predictable.
pub struct PatternEntropy {
    seed: u8,
    counter: Mutex<u8>,
}

impl PatternEntropy {
    pub fn new(seed: u8) -> Self {
        Self { seed, counter: Mutex::new(0) }
    }
}

impl Entropy for PatternEntropy {
    fn fill(&self, buf: &mut [u8]) {
        let mut n = self.counter.lock().unwrap();
        for (i, b) in buf.iter_mut().enumerate() {
            *b = self.seed.wrapping_add(*n).wrapping_add(i as u8);
        }
        *n = n.wrapping_add(1);
    }
}

/// Always hands out the same bytes (repeated to fit), for vectors recorded with fixed nonces.
pub struct FixedEntropy(pub u8);

impl Entropy for FixedEntropy {
    fn fill(&self, buf: &mut [u8]) {
        buf.fill(self.0);
    }
}

#[derive(Default)]
pub struct MemoryKeyPairStore(Mutex<Option<KeyPair>>);

impl KeyPairStore for MemoryKeyPairStore {
    fn load(&self) -> Result<Option<KeyPair>, StoreError> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn save(&self, pair: &KeyPair) -> Result<(), StoreError> {
        *self.0.lock().unwrap() = Some(pair.clone());
        Ok(())
    }
}

#[derive(Default)]
pub struct MemoryDevices(Mutex<Vec<Device>>);

impl DeviceRegistry for MemoryDevices {
    fn find_by_token(&self, token: &str) -> Option<Device> {
        self.0.lock().unwrap().iter().find(|d| d.token == token).cloned()
    }
    fn register(&self, device: Device) -> Result<(), StoreError> {
        self.0.lock().unwrap().push(device);
        Ok(())
    }
    fn revoke(&self, device_id: &str) -> Result<bool, StoreError> {
        let mut devices = self.0.lock().unwrap();
        let before = devices.len();
        devices.retain(|d| d.id != device_id);
        Ok(devices.len() != before)
    }
}
