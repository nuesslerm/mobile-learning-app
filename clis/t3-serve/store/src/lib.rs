//! SQLite for the event log (one global sequence by AUTOINCREMENT), command receipts, and the hashed auth records. WAL and a busy
//! timeout let `wf t3 pair` write a grant while `wf t3 serve` runs on the same file.
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use std::path::Path;
use std::sync::Mutex;
use t3_core::ports::*;

pub struct SqliteStore(Mutex<Connection>);

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS events (sequence INTEGER PRIMARY KEY AUTOINCREMENT, event_id TEXT NOT NULL DEFAULT '', kind TEXT NOT NULL, thread_id TEXT, project_id TEXT, occurred_at INTEGER NOT NULL, payload TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS events_thread ON events (thread_id, sequence);
CREATE TABLE IF NOT EXISTS receipts (command_id TEXT PRIMARY KEY, command_type TEXT NOT NULL, sequence INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS grants (hash TEXT PRIMARY KEY, scopes TEXT NOT NULL, label TEXT NOT NULL, expires_at INTEGER NOT NULL, used_at INTEGER);
CREATE TABLE IF NOT EXISTS tokens (hash TEXT PRIMARY KEY, id TEXT NOT NULL, scopes TEXT NOT NULL, label TEXT NOT NULL, expires_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS tickets (hash TEXT PRIMARY KEY, token_id TEXT NOT NULL, scopes TEXT NOT NULL, use TEXT NOT NULL, expires_at INTEGER NOT NULL, used_at INTEGER);
";

fn err(e: impl std::fmt::Display) -> StoreError {
    StoreError(e.to_string())
}

fn scopes_text(scopes: &[String]) -> String {
    Value::from(scopes.to_vec()).to_string()
}

fn scopes_of(text: &str) -> Vec<String> {
    serde_json::from_str(text).unwrap_or_default()
}

impl SqliteStore {
    /// Opens (creating, mode 0600) the database at `path`.
    pub fn open(path: &Path) -> StoreResult<SqliteStore> {
        let fresh = !path.exists();
        let conn = Connection::open(path).map_err(err)?;
        if fresh {
            #[cfg(unix)]
            std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o600)).map_err(err)?;
        }
        conn.busy_timeout(std::time::Duration::from_secs(5)).map_err(err)?;
        conn.pragma_update(None, "journal_mode", "WAL").map_err(err)?;
        conn.pragma_update(None, "synchronous", "NORMAL").map_err(err)?;
        conn.execute_batch(SCHEMA).map_err(err)?;
        // a database from before event ids: the column is added, old rows keep an empty id
        let _ = conn.execute("ALTER TABLE events ADD COLUMN event_id TEXT NOT NULL DEFAULT ''", []);
        Ok(SqliteStore(Mutex::new(conn)))
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }
}

fn event_of(row: &rusqlite::Row) -> rusqlite::Result<StoredEvent> {
    let payload: String = row.get(6)?;
    Ok(StoredEvent { sequence: row.get::<_, i64>(0)? as u64, event_id: row.get(1)?, kind: row.get(2)?, thread_id: row.get(3)?, project_id: row.get(4)?, occurred_at: row.get(5)?, payload: serde_json::from_str(&payload).unwrap_or(Value::Null) })
}

impl EventStore for SqliteStore {
    fn append(&self, drafts: Vec<EventDraft>, receipt: Option<(&str, &str)>) -> StoreResult<Vec<StoredEvent>> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(err)?;
        let mut out = Vec::new();
        for d in drafts {
            tx.execute("INSERT INTO events (event_id, kind, thread_id, project_id, occurred_at, payload) VALUES (?6, ?1, ?2, ?3, ?4, ?5)", params![d.kind, d.thread_id, d.project_id, d.occurred_at, d.payload.to_string(), d.event_id]).map_err(err)?;
            out.push(StoredEvent { sequence: tx.last_insert_rowid() as u64, event_id: d.event_id, kind: d.kind, thread_id: d.thread_id, project_id: d.project_id, occurred_at: d.occurred_at, payload: d.payload });
        }
        if let (Some((id, kind)), Some(last)) = (receipt, out.last()) {
            tx.execute("INSERT OR IGNORE INTO receipts (command_id, command_type, sequence) VALUES (?1, ?2, ?3)", params![id, kind, last.sequence as i64]).map_err(err)?;
        }
        tx.commit().map_err(err)?;
        Ok(out)
    }

    fn read_after(&self, after: u64) -> StoreResult<Vec<StoredEvent>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT sequence, event_id, kind, thread_id, project_id, occurred_at, payload FROM events WHERE sequence > ?1 ORDER BY sequence").map_err(err)?;
        let rows = stmt.query_map([after as i64], event_of).map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }

    fn last_sequence(&self) -> StoreResult<u64> {
        let last: Option<i64> = self.conn().query_row("SELECT MAX(sequence) FROM events", [], |r| r.get(0)).map_err(err)?;
        Ok(last.unwrap_or(0) as u64)
    }

    fn receipt(&self, command_id: &str) -> StoreResult<Option<Receipt>> {
        self.conn().query_row("SELECT command_type, sequence FROM receipts WHERE command_id = ?1", [command_id], |r| Ok(Receipt { command_type: r.get(0)?, sequence: r.get::<_, i64>(1)? as u64 })).optional().map_err(err)
    }
}

impl AuthStore for SqliteStore {
    fn put_grant(&self, hash: &str, grant: &GrantRecord, expires_at: Millis) -> StoreResult<()> {
        self.conn().execute("INSERT INTO grants (hash, scopes, label, expires_at) VALUES (?1, ?2, ?3, ?4)", params![hash, scopes_text(&grant.scopes), grant.label, expires_at]).map_err(err)?;
        Ok(())
    }

    fn consume_grant(&self, hash: &str, now: Millis) -> StoreResult<Result<GrantRecord, GrantRefusal>> {
        let conn = self.conn();
        let claimed = conn.execute("UPDATE grants SET used_at = ?2 WHERE hash = ?1 AND used_at IS NULL AND expires_at > ?2", params![hash, now]).map_err(err)?;
        let row = conn.query_row("SELECT scopes, label, used_at FROM grants WHERE hash = ?1", [hash], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<i64>>(2)?))).optional().map_err(err)?;
        Ok(match (claimed, row) {
            (_, None) => Err(GrantRefusal::Unknown),
            (1, Some((scopes, label, _))) => Ok(GrantRecord { scopes: scopes_of(&scopes), label }),
            (_, Some((_, _, Some(_)))) => Err(GrantRefusal::Used),
            (_, Some(_)) => Err(GrantRefusal::Expired),
        })
    }

    fn put_token(&self, hash: &str, token: &TokenRecord) -> StoreResult<()> {
        self.conn().execute("INSERT INTO tokens (hash, id, scopes, label, expires_at) VALUES (?1, ?2, ?3, ?4, ?5)", params![hash, token.id, scopes_text(&token.scopes), token.label, token.expires_at]).map_err(err)?;
        Ok(())
    }

    fn find_token(&self, hash: &str) -> StoreResult<Option<TokenRecord>> {
        self.conn()
            .query_row("SELECT id, scopes, label, expires_at FROM tokens WHERE hash = ?1", [hash], |r| Ok(TokenRecord { id: r.get(0)?, scopes: scopes_of(&r.get::<_, String>(1)?), label: r.get(2)?, expires_at: r.get(3)? }))
            .optional()
            .map_err(err)
    }

    fn put_ticket(&self, hash: &str, ticket: &TicketRecord, expires_at: Millis) -> StoreResult<()> {
        self.conn().execute("INSERT INTO tickets (hash, token_id, scopes, use, expires_at) VALUES (?1, ?2, ?3, ?4, ?5)", params![hash, ticket.token_id, scopes_text(&ticket.scopes), ticket.use_, expires_at]).map_err(err)?;
        Ok(())
    }

    fn consume_ticket(&self, hash: &str, now: Millis) -> StoreResult<Option<TicketRecord>> {
        let conn = self.conn();
        let claimed = conn.execute("UPDATE tickets SET used_at = ?2 WHERE hash = ?1 AND used_at IS NULL AND expires_at > ?2", params![hash, now]).map_err(err)?;
        if claimed != 1 {
            return Ok(None);
        }
        conn.query_row("SELECT token_id, scopes, use FROM tickets WHERE hash = ?1", [hash], |r| Ok(TicketRecord { token_id: r.get(0)?, scopes: scopes_of(&r.get::<_, String>(1)?), use_: r.get(2)? })).optional().map_err(err)
    }
}

impl MetaStore for SqliteStore {
    fn get(&self, key: &str) -> StoreResult<Option<String>> {
        self.conn().query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)).optional().map_err(err)
    }

    fn set(&self, key: &str, value: &str) -> StoreResult<()> {
        self.conn().execute("INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![key, value]).map_err(err)?;
        Ok(())
    }
}

impl StorageProbe for SqliteStore {
    fn sample(&self) -> Result<StorageSample, String> {
        let conn = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let number = |pragma: &str| conn.query_row(&format!("PRAGMA {pragma}"), [], |row| row.get::<_, i64>(0)).map(|n| n.max(0) as u64).map_err(|e| e.to_string());
        let page = number("page_size")?;
        let path = conn.query_row("SELECT file FROM pragma_database_list WHERE name = 'main'", [], |row| row.get::<_, String>(0)).map_err(|e| e.to_string())?;
        Ok(StorageSample { database_bytes: number("page_count")? * page, reclaimable_bytes: number("freelist_count")? * page, incremental_vacuum: number("auto_vacuum")? == 2, path })
    }
}
