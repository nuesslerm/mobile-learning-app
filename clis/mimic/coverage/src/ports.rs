//! What the core reads from the world: the repository tree and its git history. Paths are relative to the repository root and
//! use `/`.

pub trait Tree {
    /// The file's text, or `None` when it is missing or unreadable.
    fn read(&self, path: &str) -> Option<String>;
    /// Every file under `dir`, recursively and sorted, skipping `node_modules`; empty when `dir` is missing.
    fn files(&self, dir: &str) -> Vec<String>;
    fn is_file(&self, path: &str) -> bool;
}

pub trait History {
    /// The commit a checked-out reference (a submodule directory) is at, or `None` when it is no checkout.
    fn checkout_head(&self, dir: &str) -> Option<String>;
    /// `path` as committed at `rev`, or `None` when the revision or the file does not exist there.
    fn show(&self, rev: &str, path: &str) -> Option<String>;
    /// Whether `path` differs from HEAD in the working tree.
    fn is_dirty(&self, path: &str) -> bool;
    /// The full messages of the commits in `base..HEAD`.
    fn messages_since(&self, base: &str) -> Vec<String>;
}

pub trait Traces {
    /// The method-hit traces `--traces` names, each as `(where it was read, its text)`: a test output directory (every `trace.json` under it),
    /// one trace file, a bundle's `bundle.json` or its URL (the files its `traces` list). An error names what could not be read.
    fn load(&self, source: &str) -> Result<Vec<(String, String)>, String>;
}

pub trait Evidence {
    /// Whether a call whose trace labels it `diff`, replayed from a cassette of provenance `cassette`, proves a row decided `oracle: mimic`.
    /// The vocabulary of the labels belongs to the oracle's tool, not to this crate.
    fn admits(&self, cassette: Option<&str>, diff: Option<&str>) -> bool;
}

pub trait Upstream {
    /// The body of a GET of `url` (redirects followed). An error names the URL and what went wrong, a non-success status included.
    fn get(&self, url: &str) -> Result<String, String>;
}
