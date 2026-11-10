//! E2EE v2 frames: `nonce(24) || secretbox(header || payload)`, the nonce and header both derived from session, direction, kind and counter,
//! so a replayed, reordered, redirected or re-kinded frame fails before the box is opened.
use crate::boxes::{secretbox_open, secretbox_seal, KEY_LEN, NONCE_LEN, TAG_LEN};

pub const SESSION_ID_LEN: usize = 32;
const HEADER_LEN: usize = SESSION_ID_LEN + 1 + 1 + 8;
const FRAME_VERSION: u8 = 2;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    MobileToDesktop,
    DesktopToMobile,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Text,
    Binary,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameRefusal {
    TooShort,
    /// Wrong session, direction, kind or counter: a replay, a reorder or a frame for another socket.
    NotExpected,
    Unauthentic,
    HeaderMismatch,
}

fn header(session_id: &[u8; SESSION_ID_LEN], direction: Direction, kind: Kind, counter: u64) -> [u8; HEADER_LEN] {
    let mut h = [0u8; HEADER_LEN];
    h[..SESSION_ID_LEN].copy_from_slice(session_id);
    h[SESSION_ID_LEN] = direction as u8;
    h[SESSION_ID_LEN + 1] = kind as u8;
    h[SESSION_ID_LEN + 2..].copy_from_slice(&counter.to_be_bytes());
    h
}

fn nonce(session_id: &[u8; SESSION_ID_LEN], direction: Direction, kind: Kind, counter: u64) -> [u8; NONCE_LEN] {
    let mut n = [0u8; NONCE_LEN];
    n[..12].copy_from_slice(&session_id[..12]);
    n[12] = FRAME_VERSION;
    n[13] = direction as u8;
    n[14] = kind as u8;
    n[16..].copy_from_slice(&counter.to_be_bytes());
    n
}

pub fn seal(
    key: &[u8; KEY_LEN],
    session_id: &[u8; SESSION_ID_LEN],
    direction: Direction,
    kind: Kind,
    counter: u64,
    payload: &[u8],
) -> Vec<u8> {
    let nonce = nonce(session_id, direction, kind, counter);
    let mut plaintext = header(session_id, direction, kind, counter).to_vec();
    plaintext.extend_from_slice(payload);
    let mut frame = nonce.to_vec();
    frame.extend(secretbox_seal(key, &nonce, &plaintext));
    frame
}

pub fn open(
    key: &[u8; KEY_LEN],
    session_id: &[u8; SESSION_ID_LEN],
    direction: Direction,
    kind: Kind,
    expected_counter: u64,
    frame: &[u8],
) -> Result<Vec<u8>, FrameRefusal> {
    if frame.len() < NONCE_LEN + TAG_LEN + HEADER_LEN {
        return Err(FrameRefusal::TooShort);
    }
    let expected = nonce(session_id, direction, kind, expected_counter);
    if frame[..NONCE_LEN] != expected {
        return Err(FrameRefusal::NotExpected);
    }
    let plaintext = secretbox_open(key, &expected, &frame[NONCE_LEN..]).ok_or(FrameRefusal::Unauthentic)?;
    if plaintext[..HEADER_LEN] != header(session_id, direction, kind, expected_counter) {
        return Err(FrameRefusal::HeaderMismatch);
    }
    Ok(plaintext[HEADER_LEN..].to_vec())
}
