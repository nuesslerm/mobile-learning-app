//! In-memory doubles of the core's ports.
use crate::ports::{Evidence, History, Traces, Tree, Upstream};
use std::collections::BTreeMap;

#[derive(Default, Clone)]
pub struct MemoryTree {
    pub files: BTreeMap<String, String>,
}

impl MemoryTree {
    pub fn of(files: &[(&str, &str)]) -> MemoryTree {
        MemoryTree { files: files.iter().map(|(p, t)| (p.to_string(), t.to_string())).collect() }
    }
}

impl Tree for MemoryTree {
    fn read(&self, path: &str) -> Option<String> {
        self.files.get(path).cloned()
    }
    fn files(&self, dir: &str) -> Vec<String> {
        let prefix = format!("{}/", dir.trim_end_matches('/'));
        self.files.keys().filter(|p| p.starts_with(&prefix) && !p.contains("/node_modules/")).cloned().collect()
    }
    fn is_file(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }
}

/// A history: reference checkouts by directory, committed files by `rev:path`, dirty paths and the messages since each base.
#[derive(Default, Clone)]
pub struct MemoryHistory {
    pub heads: BTreeMap<String, String>,
    pub committed: BTreeMap<String, String>,
    pub dirty: Vec<String>,
    pub messages: BTreeMap<String, Vec<String>>,
}

impl History for MemoryHistory {
    fn checkout_head(&self, dir: &str) -> Option<String> {
        self.heads.get(dir).cloned()
    }
    fn show(&self, rev: &str, path: &str) -> Option<String> {
        self.committed.get(&format!("{rev}:{path}")).cloned()
    }
    fn is_dirty(&self, path: &str) -> bool {
        self.dirty.iter().any(|d| d == path)
    }
    fn messages_since(&self, base: &str) -> Vec<String> {
        self.messages.get(base).cloned().unwrap_or_default()
    }
}

/// Traces by the source they are asked for as; an unknown source is an error.
#[derive(Default, Clone)]
pub struct MemoryTraces {
    pub sources: BTreeMap<String, Vec<(String, String)>>,
}

impl Traces for MemoryTraces {
    fn load(&self, source: &str) -> Result<Vec<(String, String)>, String> {
        self.sources.get(source).cloned().ok_or_else(|| format!("{source}: no traces"))
    }
}

/// An oracle that admits the `(cassette, diff)` pairs it is given and nothing else.
#[derive(Default, Clone)]
pub struct AdmitOnly {
    pub pairs: Vec<(String, String)>,
}

impl AdmitOnly {
    pub fn of(pairs: &[(&str, &str)]) -> AdmitOnly {
        AdmitOnly { pairs: pairs.iter().map(|(c, d)| (c.to_string(), d.to_string())).collect() }
    }
}

impl Evidence for AdmitOnly {
    fn admits(&self, cassette: Option<&str>, diff: Option<&str>) -> bool {
        self.pairs.iter().any(|(c, d)| Some(c.as_str()) == cassette && Some(d.as_str()) == diff)
    }
}

/// Upstream answers by URL; a URL it does not know is a failed request, and every URL asked is kept in order.
#[derive(Default)]
pub struct FakeUpstream {
    pub answers: BTreeMap<String, String>,
    pub asked: std::cell::RefCell<Vec<String>>,
}

impl FakeUpstream {
    pub fn of(answers: &[(&str, &str)]) -> FakeUpstream {
        FakeUpstream { answers: answers.iter().map(|(u, b)| (u.to_string(), b.to_string())).collect(), ..Default::default() }
    }
}

impl Upstream for FakeUpstream {
    fn get(&self, url: &str) -> Result<String, String> {
        self.asked.borrow_mut().push(url.to_string());
        self.answers.get(url).cloned().ok_or_else(|| format!("{url}: answered 404"))
    }
}
