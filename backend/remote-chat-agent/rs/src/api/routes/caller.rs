//! Who is calling, for the rate limit: Express's `req.ip` with `trust proxy: 'loopback'`. Walking from the socket address outwards
//! through `X-Forwarded-For`, the first address that is not a loopback proxy is the caller; all loopback gives the last one walked.
//! A peer that is not loopback is the caller, whatever it forwards.

use axum::extract::ConnectInfo;
use axum::http::Request;
use std::net::{IpAddr, SocketAddr};
use tower_governor::key_extractor::KeyExtractor;
use tower_governor::GovernorError;

fn trusted(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => v6.is_loopback() || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback()),
    }
}

pub fn client_address(peer: IpAddr, forwarded: Option<&str>) -> String {
    let peer_text = peer.to_string();
    if !trusted(&peer) {
        return peer_text;
    }
    let mut chain = vec![peer_text];
    if let Some(forwarded) = forwarded {
        chain.extend(forwarded.split(',').map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).rev());
    }
    for (i, address) in chain.iter().enumerate().take(chain.len() - 1) {
        let is_trusted = address.parse::<IpAddr>().is_ok_and(|ip| trusted(&ip));
        if !is_trusted {
            return chain[i].clone();
        }
    }
    chain.last().cloned().unwrap_or_default()
}

#[derive(Clone, Copy)]
pub struct Caller;

impl KeyExtractor for Caller {
    type Key = String;

    fn extract<T>(&self, req: &Request<T>) -> Result<String, GovernorError> {
        let ConnectInfo(peer) = req.extensions().get::<ConnectInfo<SocketAddr>>().ok_or(GovernorError::UnableToExtractKey)?;
        let forwarded = req.headers().get("x-forwarded-for").and_then(|v| v.to_str().ok());
        Ok(client_address(peer.ip(), forwarded))
    }
}
