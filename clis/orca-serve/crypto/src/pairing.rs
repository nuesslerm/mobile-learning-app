//! The pairing offer the phone scans: JSON in an `orca://pair?code=<base64url>` link, as the reference's `encodePairingOffer` writes it.
use crate::b64;
use serde::{Deserialize, Serialize};

pub const OFFER_VERSION: u8 = 2;
const CODE_MAX: usize = 128 * 1024;
const ENDPOINT_MAX: usize = 16 * 1024;
const TOKEN_MAX: usize = 64 * 1024;
const KEY_MAX: usize = 4 * 1024;
const DEVICE_ID_MAX: usize = 128;
const URI_PREFIX: &str = "orca://pair?code=";

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Mobile,
    Runtime,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingOffer {
    pub v: u8,
    pub endpoint: String,
    pub device_token: String,
    pub public_key_b64: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paired_device_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OfferRefusal {
    NotAPairingLink,
    BadCode,
    BadOffer,
    /// Relay offers route through a cloud this host does not run; a direct host never emits one.
    RelayUnsupported,
}

impl PairingOffer {
    pub fn direct(endpoint: &str, device: &crate::ports::Device, host_public: &[u8; 32]) -> Self {
        Self {
            v: OFFER_VERSION,
            endpoint: endpoint.to_string(),
            device_token: device.token.clone(),
            public_key_b64: b64::encode(host_public),
            paired_device_id: Some(device.id.clone()),
            scope: Some(Scope::Mobile),
        }
    }

    pub fn to_uri(&self) -> String {
        let json = serde_json::to_vec(self).expect("an offer always serialises");
        format!("{URI_PREFIX}{}", b64::encode_url(&json))
    }

    /// Accepts the deep link or the bare code, as the phone's paste box does.
    pub fn parse(input: &str) -> Result<Self, OfferRefusal> {
        let input = input.trim();
        let code = match input.strip_prefix(URI_PREFIX) {
            Some(code) => code,
            None if input.contains("://") => return Err(OfferRefusal::NotAPairingLink),
            None => input,
        };
        if code.len() > CODE_MAX {
            return Err(OfferRefusal::BadCode);
        }
        let json = b64::decode_pairing_code(code).ok_or(OfferRefusal::BadCode)?;
        let value: serde_json::Value = serde_json::from_slice(&json).map_err(|_| OfferRefusal::BadOffer)?;
        if value.get("relay").is_some() {
            return Err(OfferRefusal::RelayUnsupported);
        }
        let offer: Self = serde_json::from_value(value).map_err(|_| OfferRefusal::BadOffer)?;
        let sized = |s: &str, max: usize| !s.is_empty() && s.len() <= max;
        let ok = offer.v == OFFER_VERSION
            && sized(&offer.endpoint, ENDPOINT_MAX)
            && sized(&offer.device_token, TOKEN_MAX)
            && sized(&offer.public_key_b64, KEY_MAX)
            && offer.paired_device_id.as_deref().is_none_or(|id| sized(id, DEVICE_ID_MAX));
        ok.then_some(offer).ok_or(OfferRefusal::BadOffer)
    }

    pub fn host_public(&self) -> Option<[u8; 32]> {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD.decode(&self.public_key_b64).ok()?;
        bytes.try_into().ok()
    }
}
