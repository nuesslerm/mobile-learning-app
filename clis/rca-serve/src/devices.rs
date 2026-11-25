use rca_mobile_core::{
    pairing::{CredentialStore, SecretSource},
    Failure, Result,
};
use rusqlite::{params, Connection, TransactionBehavior};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub struct SqlDevices(PathBuf);
pub struct OsSecrets;
impl SecretSource for OsSecrets {
    fn generate(&self) -> String {
        Uuid::new_v4().simple().to_string()
    }
}

fn hash(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}
fn store_error(error: rusqlite::Error) -> Failure {
    Failure::new("STORE_ERROR", error.to_string())
}

impl SqlDevices {
    pub fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir).map_err(|e| Failure::new("STORE_ERROR", e.to_string()))?;
        let store = Self(dir.join("mobile.sqlite"));
        store.db()?.execute_batch("CREATE TABLE IF NOT EXISTS invites (hash TEXT PRIMARY KEY, expires INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS devices (hash TEXT PRIMARY KEY);").map_err(store_error)?;
        Ok(store)
    }

    fn db(&self) -> Result<Connection> {
        Connection::open(&self.0).map_err(store_error)
    }
}

impl CredentialStore for SqlDevices {
    fn save_invite(&self, code: &str, ttl: i64) -> Result<()> {
        let db = self.db()?;
        db.execute("DELETE FROM invites WHERE expires <= unixepoch()", [])
            .map_err(store_error)?;
        db.execute(
            "INSERT INTO invites(hash,expires) VALUES(?1,unixepoch()+?2)",
            params![hash(code), ttl],
        )
        .map_err(store_error)?;
        Ok(())
    }
    fn redeem_once(&self, code: &str, token: &str) -> Result<bool> {
        let mut db = self.db()?;
        let transaction = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store_error)?;
        let redeemed = transaction
            .execute(
                "DELETE FROM invites WHERE hash=?1 AND expires>unixepoch()",
                [hash(code)],
            )
            .map_err(store_error)?;
        if redeemed == 0 {
            return Ok(false);
        }
        transaction
            .execute("INSERT INTO devices(hash) VALUES(?1)", params![hash(token)])
            .map_err(store_error)?;
        transaction.commit().map_err(store_error)?;
        Ok(true)
    }

    fn authenticated(&self, token: &str) -> Result<bool> {
        self.db()?
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM devices WHERE hash=?1)",
                [hash(token)],
                |r| r.get(0),
            )
            .map_err(store_error)
    }

    fn revoke(&self, token: &str) -> Result<()> {
        self.db()?
            .execute("DELETE FROM devices WHERE hash=?1", [hash(token)])
            .map_err(store_error)?;
        Ok(())
    }
}
