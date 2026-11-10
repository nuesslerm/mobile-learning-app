//! The shared cooldown table as the ladder's second store: a fake ORDS endpoint holds the rows, and the adapter is driven
//! through the `CooldownStore` port only.
use rca_ladder::ords_store::{OrdsLedgerStore, OrdsReply, OrdsTransport};
use rca_ladder::pool_ledger::{Entry, Ledger};
use rca_ladder::store::CooldownStore;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

#[derive(Clone, Default)]
struct Table {
    rows: Rc<RefCell<BTreeMap<String, String>>>,
    requests: Rc<RefCell<Vec<String>>>,
    down: Rc<RefCell<Option<u16>>>,
}

impl OrdsTransport for Table {
    fn send(&self, method: &str, url: &str, body: Option<&str>) -> Result<OrdsReply, String> {
        self.requests.borrow_mut().push(format!("{method} {url}"));
        if let Some(status) = *self.down.borrow() {
            return Ok(OrdsReply { status, body: "{\"title\":\"unavailable\"}".into() });
        }
        let key = url.rsplit('/').next().unwrap_or_default().to_string();
        let ok = |body: String| Ok(OrdsReply { status: 200, body });
        match method {
            "GET" => {
                let items: Vec<Value> = self.rows.borrow().iter().map(|(pool, entry)| json!({ "pool": pool, "entry": entry })).collect();
                ok(json!({ "items": items, "hasMore": false }).to_string())
            }
            "PUT" => {
                let row: Value = serde_json::from_str(body.unwrap()).unwrap();
                assert!(row["updated_at"].is_string(), "ORDS binds an omitted column to NULL, so every column is sent");
                self.rows.borrow_mut().insert(key, row["entry"].as_str().unwrap().to_string());
                ok("{}".into())
            }
            "DELETE" => {
                self.rows.borrow_mut().remove(&key);
                ok("{}".into())
            }
            other => Err(format!("unexpected {other}")),
        }
    }
}

fn cold(until: &str) -> Entry {
    Entry { cold_until: until.into(), learned_reset_ms: 18_000_000, last_cap_at: "2026-10-27T10:00:00.000Z".into(), last_timeout_at: None, cold_source: None, consecutive_timeouts: None }
}

const BASE: &str = "https://ords.example/ords/health/pool_cooldowns/";

#[test]
fn given_two_callers_when_each_caps_a_different_pool_then_both_caps_are_in_the_table() {
    // Given one table and two callers that both read it empty
    let table = Table::default();
    let (a, b) = (OrdsLedgerStore::new(BASE, table.clone()), OrdsLedgerStore::new(BASE, table.clone()));
    a.read();
    b.read();
    // When each writes a ledger holding only the pool it capped
    a.write(&Ledger::from([("kimi".to_string(), cold("2026-10-27T15:00:00.000Z"))])).unwrap();
    b.write(&Ledger::from([("minimax".to_string(), cold("2026-10-27T16:00:00.000Z"))])).unwrap();
    // Then a fresh reader sees both pools
    let seen = OrdsLedgerStore::new(BASE, table.clone()).read();
    assert_eq!(seen.keys().cloned().collect::<Vec<_>>(), vec!["kimi", "minimax"]);
    assert_eq!(seen["kimi"], cold("2026-10-27T15:00:00.000Z"));
}

#[test]
fn given_a_pool_that_recovered_when_the_ledger_is_written_then_only_its_row_is_deleted() {
    // Given a table with two capped pools, read by the caller
    let table = Table::default();
    let store = OrdsLedgerStore::new(BASE, table.clone());
    store.write(&Ledger::from([("kimi".into(), cold("2026-10-27T15:00:00.000Z")), ("minimax".into(), cold("2026-10-27T16:00:00.000Z"))])).unwrap();
    let ledger = store.read();
    table.requests.borrow_mut().clear();
    // When the caller clears one pool and writes the ledger back
    let mut next = ledger.clone();
    next.remove("kimi");
    store.write(&next).unwrap();
    // Then one DELETE for that pool is the only request
    assert_eq!(*table.requests.borrow(), vec![format!("DELETE {}/kimi", BASE.trim_end_matches('/'))]);
    assert_eq!(table.rows.borrow().keys().cloned().collect::<Vec<_>>(), vec!["minimax"]);
}

#[test]
fn given_the_database_is_down_when_the_ledger_is_read_then_it_is_empty_and_says_why_and_a_write_fails() {
    // Given an endpoint that answers 503
    let table = Table::default();
    *table.down.borrow_mut() = Some(503);
    let store = OrdsLedgerStore::new(BASE, table);
    // When the ledger is read and written
    let read = store.read();
    let written = store.write(&Ledger::from([("kimi".into(), cold("2026-10-27T15:00:00.000Z"))]));
    // Then the read is empty with the status as its reason, and the write is an error
    assert!(read.is_empty());
    assert!(store.last_error().unwrap().contains("503"));
    assert!(written.unwrap_err().to_string().contains("503"));
}

#[test]
fn given_a_pool_name_that_is_not_a_url_segment_when_written_then_it_is_refused_before_any_request() {
    // Given a ledger with a pool name holding a slash
    let table = Table::default();
    let store = OrdsLedgerStore::new(BASE, table.clone());
    // When it is written
    let written = store.write(&Ledger::from([("a/b".into(), cold("2026-10-27T15:00:00.000Z"))]));
    // Then it is refused and nothing was sent
    assert!(written.is_err());
    assert!(table.requests.borrow().is_empty());
}
