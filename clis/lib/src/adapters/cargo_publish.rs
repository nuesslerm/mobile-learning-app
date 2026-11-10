//! Any registry with Cargo's publish API, crates.io included: `cargo publish`, with the registry and its token given through the
//! environment of that one child so neither is written to a file or printed.
use crate::domain::libraries::Registry;
use crate::ports::{Failure, Published, Publisher, Release};
use std::process::{Command, Stdio};

pub struct CargoPublish {
    registry: Registry,
    index_url: Option<String>,
    token: String,
}

impl CargoPublish {
    pub fn new(registry: Registry, index_url: Option<String>, token: String) -> CargoPublish {
        CargoPublish { registry, index_url, token }
    }

    fn is_crates_io(&self) -> bool {
        self.registry.kind == "crates-io"
    }

    fn scrub(&self, text: &str) -> String {
        let mut clean = text.replace(&self.token, "<token>");
        if let Some(url) = &self.index_url {
            clean = clean.replace(url.trim_start_matches("sparse+"), "<registry>");
        }
        clean
    }
}

impl Publisher for CargoPublish {
    fn id(&self) -> &'static str {
        "cargo-publish"
    }

    fn already_released(&self, _: &str, _: &str) -> Result<bool, Failure> {
        Ok(false)
    }

    fn publish(&self, release: &Release) -> Result<Published, Failure> {
        let scratch = tempfile::Builder::new().prefix("wf-lib-").tempdir().map_err(|e| Failure::new(e.to_string()))?;
        let mut command = Command::new("cargo");
        command.args(["publish", "--allow-dirty", "--quiet", "--manifest-path"]).arg(release.manifest).env("CARGO_TARGET_DIR", scratch.path());
        if self.is_crates_io() {
            command.env("CARGO_REGISTRY_TOKEN", &self.token);
        } else {
            let key = self.registry.alias.to_ascii_uppercase().replace('-', "_");
            command.args(["--registry", &self.registry.alias]).env(format!("CARGO_REGISTRIES_{key}_TOKEN"), &self.token);
            if let Some(url) = &self.index_url {
                let url = if url.starts_with("sparse+") || url.starts_with("registry+") { url.clone() } else { format!("sparse+{url}") };
                command.env(format!("CARGO_REGISTRIES_{key}_INDEX"), url);
            }
        }
        let out = command.stdin(Stdio::null()).output().map_err(|e| Failure::new(format!("cargo publish: {e}")))?;
        if out.status.success() {
            return Ok(Published::default());
        }
        let said = self.scrub(&String::from_utf8_lossy(&out.stderr));
        let tail: Vec<&str> = said.lines().rev().take(10).collect();
        Err(Failure::new(format!("cargo publish to {} failed: {}", self.registry.alias, tail.into_iter().rev().collect::<Vec<_>>().join(" | "))).next("fix what cargo reports; the manifest version was restored"))
    }
}
