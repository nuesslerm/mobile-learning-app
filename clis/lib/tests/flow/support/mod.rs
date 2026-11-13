//! A repository to run `wf lib` in, and the doubles around it.
pub use wf_lib::testing::{CargoRegistryFake, PublicReads, MemBucket};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
pub use wf_testkit::{last_json, RunResult};
use wf_testkit::{run_wf, RunOpts};

pub struct Repo {
    pub dir: tempfile::TempDir,
    pub bucket: MemBucket,
    pub proxy: PublicReads,
    pub bin: tempfile::TempDir,
    cargo_home: tempfile::TempDir,
    pub env: Vec<(String, String)>,
}

pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").arg("-C").arg(dir).args(args).stdin(Stdio::null()).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

pub fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// The `libraries.json` of the fixture: our bucket as `mla` and crates.io, one library on `mla`. `@INDEX@` becomes the read
/// public bucket's index base when the `Repo` is made.
pub fn libraries_json(registry: &str, extra_registries: &str, extra_libraries: &str) -> String {
    format!(
        r#"{{
  "registries": [
    {{ "alias": "mla", "kind": "cargo-sparse", "tokenEnv": "LIBS_WRITE_PAR_URL", "publish": "bucket-sparse", "indexUrl": "@INDEX@" }},
    {{ "alias": "crates-io", "kind": "crates-io", "url": "sparse+https://index.crates.io/", "tokenEnv": "CARGO_REGISTRY_TOKEN", "publish": "cargo-publish" }}{extra_registries}
  ],
  "libraries": [
    {{ "name": "fixture-core", "kind": "rust", "path": "libs/fixture-core", "registry": "{registry}" }}{extra_libraries}
  ]
}}
"#
    )
}

pub fn crate_manifest(name: &str, version: &str, deps: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\nedition = \"2021\"\ndescription = \"a fixture\"\nlicense = \"MIT\"\n\n[dependencies]\n{deps}")
}

impl Repo {
    /// A repository on `dev` with a bare `origin`, `libs/fixture-core` at 0.1.0 and the given `libraries.json`.
    pub fn new(libraries: &str) -> Repo {
        let bucket = MemBucket::start();
        let repo = Repo { dir: tempfile::tempdir().unwrap(), proxy: PublicReads::start(&bucket), bucket, bin: tempfile::tempdir().unwrap(), cargo_home: tempfile::tempdir().unwrap(), env: Vec::new() };
        let origin = repo.dir.path().join("origin.git");
        let work = repo.work();
        std::fs::create_dir_all(&work).unwrap();
        git(origin.parent().unwrap(), &["init", "-q", "--bare", "-b", "dev", "origin.git"]);
        git(&work, &["init", "-q", "-b", "dev"]);
        git(&work, &["config", "user.email", "t@example.com"]);
        git(&work, &["config", "user.name", "t"]);
        git(&work, &["remote", "add", "origin", origin.to_str().unwrap()]);
        write(&work.join("libraries.json"), &libraries.replace("@INDEX@", &repo.proxy.index_url()));
        write(&work.join(".gitignore"), "target/\n");
        repo.core("0.1.0", "pub fn answer() -> u32 { 42 }\n");
        repo.bucket.oci_stub(repo.bin.path());
        repo.commit("init");
        repo.push();
        repo
    }

    /// What Cargo is given as the registry index: the public bucket's index base, never a bucket link.
    pub fn proxy_index(&self) -> String {
        format!("sparse+{}", self.proxy.index_url())
    }

    pub fn work(&self) -> PathBuf {
        self.dir.path().join("work")
    }

    pub fn core(&self, version: &str, source: &str) {
        write(&self.work().join("libs/fixture-core/Cargo.toml"), &crate_manifest("fixture-core", version, ""));
        write(&self.work().join("libs/fixture-core/src/lib.rs"), source);
    }

    pub fn write(&self, path: &str, text: &str) {
        write(&self.work().join(path), text);
    }

    pub fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.work().join(path)).unwrap()
    }

    pub fn commit(&self, message: &str) {
        git(&self.work(), &["add", "-A"]);
        git(&self.work(), &["commit", "-q", "--allow-empty", "-m", message]);
    }

    /// What a release needs before it runs: everything committed and pushed.
    pub fn settle(&self) {
        self.commit("settle");
        self.push();
    }

    pub fn push(&self) {
        git(&self.work(), &["push", "-q", "origin", "dev"]);
    }

    pub fn opts(&self) -> RunOpts {
        let mut opts = RunOpts::default()
            .cwd(&self.work())
            .bin_dir(self.bin.path())
            .env("LIBS_WRITE_PAR_URL", &self.bucket.par_url())
            .env("CARGO_HOME", self.cargo_home.path().to_str().unwrap())
            .env("CARGO_HTTP_MULTIPLEXING", "false")
            .env("CARGO_REGISTRIES_MLA_INDEX", &self.proxy_index())
            .timeout(Duration::from_secs(180));
        for (k, v) in &self.env {
            opts = opts.env(k, v);
        }
        opts
    }

    pub fn lib(&self, args: &[&str]) -> RunResult {
        self.lib_with(args, self.opts())
    }

    pub fn lib_with(&self, args: &[&str], opts: RunOpts) -> RunResult {
        let mut line = vec!["lib"];
        line.extend(args);
        run_wf(&line, &opts)
    }

    /// `cargo run` of a crate in `dir` (outside this repository's own tooling), against the given registry environment.
    pub fn cargo_run(&self, manifest: &Path, env: &[(&str, &str)]) -> String {
        self.cargo_run_with(manifest, env, &[])
    }

    pub fn cargo_run_with(&self, manifest: &Path, env: &[(&str, &str)], config: &[String]) -> String {
        let mut command = Command::new("cargo");
        for setting in config {
            command.args(["--config", setting]);
        }
        command.args(["run", "--quiet", "--manifest-path"]).arg(manifest).env("CARGO_HOME", self.cargo_home.path()).env("CARGO_HTTP_MULTIPLEXING", "false").env("CARGO_TARGET_DIR", self.dir.path().join("consumer-target"));
        for (k, v) in env {
            command.env(k, v);
        }
        for proxy in ["http_proxy", "HTTP_PROXY", "https_proxy", "HTTPS_PROXY", "all_proxy", "ALL_PROXY"] {
            command.env_remove(proxy);
        }
        let out = command.stdin(Stdio::null()).output().unwrap();
        assert!(out.status.success(), "cargo run: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    use std::io::Write;
    let mut child = Command::new("sha256sum").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    String::from_utf8_lossy(&child.wait_with_output().unwrap().stdout).split_whitespace().next().unwrap().to_string()
}

/// A Cargo.lock that pins `name` at `version` from this registry (`source` carries the public bucket's index URL, as a real lock does) or as a path.
pub fn lock(name: &str, version: &str, registry: bool) -> String {
    let source = if registry { "source = \"sparse+https://libs.example/o/index/\"\n" } else { "" };
    format!("version = 4\n\n[[package]]\nname = \"{name}\"\nversion = \"{version}\"\n{source}")
}
