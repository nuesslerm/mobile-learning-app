//! The recorded set `fixtures/orca-mobile-bundle` (a `wf mimic` cassette of the pinned Orca mobile-bundle contract) as test data: the
//! replies the pinned schemas accepted, and the synthetic bundle files, generated from the recorded case input instead of committed.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub fn set_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/orca-mobile-bundle")
}

pub struct Recorded {
    interactions: Vec<Value>,
}

impl Recorded {
    pub fn load() -> Self {
        let text = std::fs::read_to_string(set_dir().join("interactions.jsonl")).expect("the recorded set's interactions");
        Self { interactions: text.lines().map(|l| serde_json::from_str(l).expect("an interaction line")).collect() }
    }

    pub fn interaction(&self, id: &str) -> &Value {
        self.interactions.iter().find(|i| i["id"] == id).unwrap_or_else(|| panic!("no recorded interaction {id}"))
    }

    fn payload(&self, id: &str) -> Value {
        self.interaction(id)["steps"][0]["payload"].clone()
    }

    pub fn manifest_result(&self) -> Value {
        self.payload("manifest")
    }

    pub fn windows(&self) -> Vec<Value> {
        self.interaction("windows")["steps"].as_array().unwrap().iter().map(|s| s["payload"].clone()).collect()
    }

    /// The old `goldens.json` shape: `manifestResult`, `windows`, `errorCodes` and `chunkBytes`.
    pub fn goldens(&self) -> Value {
        let codes = self.payload("error-codes");
        json!({"manifestResult": self.manifest_result(), "windows": self.windows(), "errorCodes": codes["codes"], "chunkBytes": codes["chunkBytes"]})
    }

    /// Writes the bundle the recorded manifest describes: each file generated from its recorded `gen`, and `manifest.json`.
    pub fn write_bundle(&self, dir: &Path) {
        for file in self.interaction("manifest")["input"]["files"].as_array().unwrap() {
            let target = dir.join(file["path"].as_str().unwrap());
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(target, generate(&file["gen"])).unwrap();
        }
        std::fs::write(dir.join("manifest.json"), serde_json::to_string_pretty(&self.manifest_result()["manifest"]).unwrap()).unwrap();
    }

    /// A fresh bundle directory a test may damage; it is removed when dropped.
    pub fn bundle(&self) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        self.write_bundle(dir.path());
        dir
    }
}

fn sha(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

/// The bytes a recorded file generator stands for (`literal`, `text`, `noise`, `empty`).
pub fn generate(gen: &Value) -> Vec<u8> {
    let bytes = || gen["bytes"].as_u64().unwrap() as usize;
    match gen["kind"].as_str().unwrap() {
        "literal" => gen["text"].as_str().unwrap().as_bytes().to_vec(),
        "text" => {
            let seed = gen["seed"].as_u64().unwrap();
            let mut out = String::new();
            let mut i = 0u64;
            while out.len() < bytes() {
                out.push_str(&format!("const v{}={};\n", i % 97, (i * seed) % 1009));
                i += 1;
            }
            out.truncate(bytes());
            out.into_bytes()
        }
        "noise" => {
            let mut out = Vec::new();
            let mut h = sha(b"noise");
            while out.len() < bytes() {
                out.extend_from_slice(&h);
                h = sha(&h);
            }
            out.truncate(bytes());
            out
        }
        "empty" => Vec::new(),
        other => panic!("unknown file generator {other}"),
    }
}
