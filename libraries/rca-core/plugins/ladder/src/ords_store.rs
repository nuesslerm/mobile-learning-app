//! The pool cooldown ledger in the shared Autonomous Database (`infra/oracle-cloud/sql/pool_cooldowns.sql`), one row per
//! pool through ORDS auto-REST, so every caller of the ladder sees the same caps. The transport is the caller's: it owns the
//! credentials and the HTTP client, this adapter owns the rows.
use crate::pool_ledger::{Entry, Ledger};
use crate::store::CooldownStore;
use rca_core::time::iso;
use serde_json::{json, Value};
use std::cell::RefCell;

pub struct OrdsReply {
    pub status: u16,
    pub body: String,
}

pub trait OrdsTransport {
    /// One authenticated request; an error is a connection failure, a non-2xx status is a reply.
    fn send(&self, method: &str, url: &str, body: Option<&str>) -> Result<OrdsReply, String>;
}

pub struct OrdsLedgerStore<T: OrdsTransport> {
    base: String,
    http: T,
    seen: RefCell<Ledger>,
    error: RefCell<Option<String>>,
}

fn safe_pool(pool: &str) -> bool {
    !pool.is_empty() && pool.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn now_iso() -> String {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
    iso(ms)
}

impl<T: OrdsTransport> OrdsLedgerStore<T> {
    pub fn new(base_url: &str, http: T) -> Self {
        Self { base: base_url.trim_end_matches('/').to_string(), http, seen: RefCell::new(Ledger::new()), error: RefCell::new(None) }
    }

    fn fetch(&self) -> Result<Ledger, String> {
        let reply = self.http.send("GET", &format!("{}/?limit=500", self.base), None)?;
        if !(200..300).contains(&reply.status) {
            return Err(format!("ORDS {}: {}", reply.status, reply.body.chars().take(200).collect::<String>()));
        }
        let rows: Value = serde_json::from_str(&reply.body).map_err(|e| format!("ORDS reply is not JSON: {e}"))?;
        let mut ledger = Ledger::new();
        for row in rows["items"].as_array().into_iter().flatten() {
            let (Some(pool), Some(entry)) = (row["pool"].as_str(), row["entry"].as_str()) else { continue };
            if let Ok(entry) = serde_json::from_str::<Entry>(entry) {
                ledger.insert(pool.to_string(), entry);
            }
        }
        Ok(ledger)
    }

    fn put(&self, pool: &str, entry: &Entry) -> Result<(), String> {
        let body = json!({ "pool": pool, "entry": serde_json::to_string(entry).map_err(|e| e.to_string())?, "updated_at": now_iso() });
        self.expect_ok(self.http.send("PUT", &format!("{}/{pool}", self.base), Some(&body.to_string()))?)
    }

    fn expect_ok(&self, reply: OrdsReply) -> Result<(), String> {
        if (200..300).contains(&reply.status) {
            Ok(())
        } else {
            Err(format!("ORDS {}: {}", reply.status, reply.body.chars().take(200).collect::<String>()))
        }
    }
}

impl<T: OrdsTransport> CooldownStore for OrdsLedgerStore<T> {
    fn read(&self) -> Ledger {
        match self.fetch() {
            Ok(ledger) => {
                *self.error.borrow_mut() = None;
                *self.seen.borrow_mut() = ledger.clone();
                ledger
            }
            Err(e) => {
                *self.error.borrow_mut() = Some(e);
                Ledger::new()
            }
        }
    }

    /// Writes only the pools that differ from what the last read saw, so two callers capping different pools do not erase
    /// each other.
    fn write(&self, ledger: &Ledger) -> std::io::Result<()> {
        let seen = self.seen.borrow().clone();
        let fail = |e: String| std::io::Error::other(e);
        for (pool, entry) in ledger {
            if !safe_pool(pool) {
                return Err(fail(format!("pool name {pool:?} cannot be a row key")));
            }
            if seen.get(pool) != Some(entry) {
                self.put(pool, entry).map_err(fail)?;
            }
        }
        for pool in seen.keys().filter(|p| !ledger.contains_key(*p)) {
            let reply = self.http.send("DELETE", &format!("{}/{pool}", self.base), None).map_err(fail)?;
            self.expect_ok(reply).map_err(fail)?;
        }
        *self.seen.borrow_mut() = ledger.clone();
        Ok(())
    }

    fn last_error(&self) -> Option<String> {
        self.error.borrow().clone()
    }
}
