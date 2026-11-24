//! Guest links: `guestShare.create` and the two routes a guest's page calls, `redeem` and `messages`. A link is off until the owner turns on the
//! `guestSharingEnabled` setting and the host names the address guests reach it at (`T3_GUEST_HOST`); with either missing the recorded
//! `disabled` or the reference's `connect_unavailable` answers, so a fresh server shares nothing. The credentials are claims signed under the
//! store's key and kept nowhere, as the reference derives them. Not built: the guest's live stream, its page and file uploads.
use super::queries::{field, QueryFailure};
use super::service::Service;
use crate::domain::command::SERVER_ONLY;
use crate::domain::decider::{GUEST_INACTIVE, GUEST_USED, GUEST_BUSY};
use crate::usecases::engine::Failure;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde_json::{json, Value};
use t3_contracts::time::iso;

pub const TTL_MS: i64 = 24 * 60 * 60 * 1000;
pub const MESSAGE_MAX: usize = 4_000;
pub const NAME_MAX: usize = 40;
/// A credential carries no expiry of its own: the share it names does, and a guest of an ended link is told it is no longer active.
const NO_EXPIRY: i64 = i64::MAX;
const INVITE: &str = "guest-invite";
const SESSION: &str = "guest-session";

/// Why a guest's request was refused: the reason a page shows and the status of the answer.
#[derive(Debug, PartialEq)]
pub struct GuestRefusal {
    pub status: u16,
    pub reason: &'static str,
    pub message: String,
}

fn refusal(reason: &'static str, message: &str) -> GuestRefusal {
    let status = match reason {
        "invalid" | "inactive" => 403,
        "empty" => 400,
        _ => 409,
    };
    GuestRefusal { status, reason, message: message.to_string() }
}

fn bad_request() -> GuestRefusal {
    GuestRefusal { status: 400, reason: "invalid", message: "The request was not valid.".into() }
}

fn create_error(reason: &str) -> QueryFailure {
    QueryFailure::Error(json!({"_tag": "GuestShareCreateError", "reason": reason}))
}

/// A name the owner reads and the agent is told: one plain line, none of the characters that reorder or hide text.
fn is_plain_line(name: &str) -> bool {
    !name.chars().any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}' | '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'))
}

impl Service {
    fn guest_dispatch(&self, command: Value) -> Result<(), Failure> {
        debug_assert!(SERVER_ONLY.contains(&command["type"].as_str().unwrap_or_default()));
        let Value::Object(body) = command else { return Ok(()) };
        self.engine().dispatch_server(body).map(|_| ())
    }

    fn command_id(&self, what: &str) -> String {
        format!("guest-{what}-{}", crate::domain::auth::uuid_v4(&self.0.ids.random_bytes(16)))
    }

    /// The thread and share a signed claim names, while that share is the thread's current one.
    fn guest_claims(&self, token: &str, kind: &str) -> Result<(String, Value), GuestRefusal> {
        let invalid = || refusal("invalid", "This guest link is not valid.");
        let claims = self.verify(token, kind).ok_or_else(invalid)?;
        let share_id = claims["shareId"].as_str().ok_or_else(invalid)?.to_string();
        let found = self.engine().read(|state| state.threads.iter().find_map(|(id, t)| t.get("guestShare").filter(|g| g["id"] == share_id).map(|g| (id.clone(), g.clone()))));
        found.ok_or_else(|| refusal("inactive", GUEST_INACTIVE))
    }

    fn sharing_on(&self) -> bool {
        self.effective_settings()["guestSharingEnabled"] == true
    }

    /// `guestShare.create`: opens a link on the thread and answers its address and the share as the thread's row now carries it.
    pub fn guest_share_create(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let thread = field(payload, "threadId")?.to_string();
        if !self.sharing_on() {
            self.log("guest.share.disabled", &[]);
            return Err(create_error("disabled"));
        }
        let Some(host) = self.0.guest_host.clone() else {
            self.log("guest.share.no_host", &[]);
            return Err(create_error("connect_unavailable"));
        };
        let share_id = URL_SAFE_NO_PAD.encode(self.0.ids.random_bytes(16));
        let expires = self.0.clock.now_ms() + TTL_MS;
        let opened = json!({"type": "thread.guest-share.open", "commandId": self.command_id("open"), "threadId": thread, "shareId": share_id, "host": host, "expiresAt": iso(expires)});
        if self.guest_dispatch(opened).is_err() {
            self.log("guest.share.thread_unavailable", &[]);
            return Err(create_error("thread_unavailable"));
        }
        let share = self.engine().read(|state| state.threads.get(&thread).and_then(|t| t.get("guestShare").cloned())).filter(|s| s["id"] == share_id);
        let Some(share) = share else { return Err(create_error("changed")) };
        let credential = self.sign(&json!({"version": 1, "kind": INVITE, "shareId": share_id, "expiresAt": NO_EXPIRY})).map_err(|e| QueryFailure::Error(t3_contracts::wire::host_error(&format!("the guest signing key could not be loaded: {e}"))))?;
        self.log("guest.share.opened", &[("thread", thread)]);
        Ok(json!({"url": format!("https://{host}/guest#{credential}"), "share": share}))
    }

    /// `POST /api/guest/redeem`: spends the invite once for a name and answers the session token the guest's cookie holds, with the share it is for.
    pub fn guest_redeem(&self, body: &Value) -> Result<(String, String), GuestRefusal> {
        let credential = body["credential"].as_str().map(str::trim).filter(|c| !c.is_empty());
        let name = body["name"].as_str().map(str::trim).filter(|n| !n.is_empty() && n.chars().count() <= NAME_MAX && is_plain_line(n));
        let (Some(credential), Some(name)) = (credential, name) else { return Err(bad_request()) };
        if !self.sharing_on() {
            return Err(refusal("inactive", GUEST_INACTIVE));
        }
        let (thread, share) = self.guest_claims(credential, INVITE)?;
        let share_id = share["id"].as_str().unwrap_or_default().to_string();
        let redeem = json!({"type": "thread.guest-share.redeem", "commandId": self.command_id("redeem"), "threadId": thread, "shareId": share_id, "guestName": name});
        self.guest_dispatch(redeem).map_err(guest_failure)?;
        let token = self.sign(&json!({"version": 1, "kind": SESSION, "shareId": share_id, "threadId": thread, "expiresAt": NO_EXPIRY})).map_err(|_| refusal("rejected", "The owner's server rejected this request."))?;
        self.log("guest.share.redeemed", &[("thread", thread)]);
        Ok((token, share_id))
    }

    /// `POST /api/guest/messages`: a message the owner must send or dismiss. Text only; a page for an older link is told to reload.
    pub fn guest_submit(&self, session: &str, share_param: &str, body: &Value) -> Result<(), GuestRefusal> {
        if !self.sharing_on() {
            return Err(refusal("inactive", GUEST_INACTIVE));
        }
        let (thread, share) = self.guest_claims(session, SESSION)?;
        let share_id = share["id"].as_str().unwrap_or_default().to_string();
        if body["shareId"] != share_id || share_param != share_id {
            return Err(refusal("stale_page", "This page is for an older guest link. Reload it to continue."));
        }
        let text = body["text"].as_str().map(str::trim).unwrap_or_default();
        if text.chars().count() > MESSAGE_MAX {
            return Err(bad_request());
        }
        if text.is_empty() {
            return Err(refusal("empty", "Write a message or attach a file."));
        }
        let submit = json!({"type": "thread.guest-message.submit", "commandId": self.command_id("submit"), "threadId": thread, "shareId": share_id, "guestMessageId": crate::domain::auth::uuid_v4(&self.0.ids.random_bytes(16)), "text": text});
        self.guest_dispatch(submit).map_err(guest_failure)?;
        self.log("guest.message.submitted", &[("thread", thread)]);
        Ok(())
    }
}

/// A guest is told the three sentences the reference names and nothing else of why the owner's server refused.
fn guest_failure(failure: Failure) -> GuestRefusal {
    let message = match &failure {
        Failure::Refused { message, .. } => message.as_str(),
        _ => "",
    };
    match message {
        m if m == GUEST_INACTIVE => refusal("inactive", GUEST_INACTIVE),
        m if m == GUEST_USED => refusal("used", GUEST_USED),
        m if m == GUEST_BUSY => refusal("busy", GUEST_BUSY),
        _ => refusal("rejected", "The owner's server rejected this request."),
    }
}

