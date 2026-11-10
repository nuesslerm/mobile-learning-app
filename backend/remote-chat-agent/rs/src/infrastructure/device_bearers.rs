//! The device bearers (`rca_agent_core::DeviceBearers`) on the state volume: one JSON file holding the book of digests, written beside the target
//! and renamed over it, readable by the service's user only. A token is 244 bits from the system's randomness and is never written: only its digest is.
use rca_agent_core::bearers::{Book, TOKEN_PREFIX};
use rca_agent_core::{Bearer, DeviceBearers, Minted};
use rca_service_core::domain::util::{iso_now, random_hex};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct BearerFile {
    path: PathBuf,
    /// One writer at a time: a mint and a revoke each read the book, change it and write it back.
    lock: Mutex<()>,
}

impl BearerFile {
    pub fn at(path: PathBuf) -> Self {
        Self { path, lock: Mutex::new(()) }
    }

    /// The book as it is on disk: empty when there is no file yet, an error when the file cannot be read or is no book.
    fn load(&self) -> Result<Book, String> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| format!("{} is no book of bearers: {e}", self.path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Book::default()),
            Err(e) => Err(format!("{}: {e}", self.path.display())),
        }
    }

    fn save(&self, book: &Book) -> Result<(), String> {
        let dir = self.path.parent().ok_or("the bearer file has no directory")?;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let tmp = dir.join(format!("device-bearers-{}.json.tmp", random_hex(4)));
        let written = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp).and_then(|mut file| file.write_all(serde_json::to_string(book).unwrap_or_default().as_bytes()));
        written.and_then(|()| std::fs::rename(&tmp, &self.path)).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            e.to_string()
        })
    }
}

impl DeviceBearers for BearerFile {
    fn mint(&self, label: &str) -> Result<Minted, String> {
        let _held = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut book = self.load()?;
        let token = format!("{TOKEN_PREFIX}{}{}", random_hex(16), random_hex(16));
        let bearer = Bearer { id: format!("bearer-{}", random_hex(6)), label: label.to_string(), created_at: iso_now() };
        book.add(bearer.clone(), &token);
        self.save(&book)?;
        Ok(Minted { bearer, token })
    }

    /// A book that cannot be read knows nobody: the socket stays shut until an operator has seen to it (the log says why).
    fn verify(&self, token: &str) -> Option<Bearer> {
        let _held = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        match self.load() {
            Ok(book) => book.verify(token),
            Err(why) => {
                tracing::warn!(event = "device_bearers.unreadable", reason = %why);
                None
            }
        }
    }

    fn list(&self) -> Result<Vec<Bearer>, String> {
        let _held = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        self.load().map(|book| book.list())
    }

    fn revoke(&self, id: &str) -> Result<bool, String> {
        let _held = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut book = self.load()?;
        let revoked = book.revoke(id);
        if revoked {
            self.save(&book)?;
        }
        Ok(revoked)
    }
}
