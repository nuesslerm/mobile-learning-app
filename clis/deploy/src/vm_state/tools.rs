//! The programs `wf vm state` runs on agent-shell: `restic` (through rclone's Object Storage backend) for the repository, `oci` once for the
//! tenancy's namespace. The restic password lives in the child's environment for the length of one call, never on a command line or in a file.
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// `${name_prefix}-vm-state` of `infra/oracle-cloud/vm-state.tf`.
pub const BUCKET: &str = "mobile-learning-vm-state";

/// restic's own words for a repository that is not there (every version prints them after an unreadable `config`).
const NO_REPOSITORY: &str = "Is there a repository at the following location?";

pub struct Out {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

/// What a program said when it failed: its stderr, short, without a line that could carry key material or the repository password.
fn said(stderr: &[u8], secret: &str) -> String {
    let text = String::from_utf8_lossy(stderr);
    let kept: Vec<&str> = text.lines().filter(|l| !l.contains("PRIVATE KEY") && (secret.is_empty() || !l.contains(secret))).collect();
    kept.join(" ").chars().take(400).collect()
}

/// Runs `program` with `env` added; the result carries the exit code, whatever it is.
pub fn exec(program: &str, args: &[&str], env: &[(String, String)], secret: &str) -> Result<Out, String> {
    let out = Command::new(program).args(args).envs(env.iter().map(|(k, v)| (k, v))).stdin(Stdio::null()).output().map_err(|e| format!("{program}: {e} (is it installed on this host?)"))?;
    Ok(Out { code: out.status.code().unwrap_or(-1), stdout: out.stdout, stderr: said(&out.stderr, secret) })
}

/// Runs `program`, giving back its stdout; any exit code but 0 is a failure that says what the tool said.
pub fn run(program: &str, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = exec(program, args, &[], "")?;
    if out.code == 0 {
        return Ok(out.stdout);
    }
    Err(format!("{program} {} exited {}: {}", args.iter().take(3).copied().collect::<Vec<_>>().join(" "), out.code, out.stderr))
}

/// The restic repository of the VM state: the versioned bucket through rclone's `oracleobjectstorage` backend with the key that
/// `~/.oci/config` holds on agent-shell. `WF_VM_STATE_REPO` points it at any other restic repository (the flow tests use a directory).
pub struct Restic {
    repo: String,
    password: String,
    env: Vec<(String, String)>,
    /// The program that lists every object version of the bucket (current and previous) as `oci os object list-object-versions` does.
    versions: Option<(String, Vec<String>)>,
    /// Holds the decrypted key rclone reads; removed with the value.
    _plain_key: Option<tempfile::TempDir>,
}

/// rclone's Object Storage backend cannot read a passphrase-protected API key (the `oci` CLI can), so it gets a copy of the key decrypted
/// into a private directory (tmpfs when there is one) for the length of the run, and a config that points at it.
fn plain_oci_config() -> Result<Option<(tempfile::TempDir, PathBuf)>, String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let home = std::env::var("HOME").map_err(|_| "HOME is not set".to_string())?;
    let text = std::fs::read_to_string(Path::new(&home).join(".oci/config")).map_err(|e| format!("~/.oci/config: {e}"))?;
    let value = |key: &str| text.lines().find_map(|l| l.trim().strip_prefix(key).and_then(|r| r.trim_start().strip_prefix('=')).map(|r| r.trim().to_string())).filter(|v| !v.is_empty());
    let Some(pass) = value("pass_phrase") else { return Ok(None) };
    let key_file = value("key_file").ok_or("~/.oci/config names no key_file")?;
    let out = Command::new("openssl").args(["pkey", "-in", &key_file, "-passin", "env:WF_OCI_KEY_PASS"]).env("WF_OCI_KEY_PASS", &pass).stdin(Stdio::null()).output().map_err(|e| format!("openssl: {e} (is it installed on this host?)"))?;
    if !out.status.success() {
        return Err(format!("openssl pkey could not decrypt {key_file}: {}", said(&out.stderr, &pass)));
    }
    let shm = Path::new("/dev/shm");
    let dir = if shm.is_dir() { tempfile::Builder::new().prefix("wf-oci-").tempdir_in(shm) } else { tempfile::Builder::new().prefix("wf-oci-").tempdir() }.map_err(|e| format!("a directory for the decrypted key: {e}"))?;
    let write = |name: &str, body: &[u8]| -> Result<PathBuf, String> {
        let path = dir.path().join(name);
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        f.write_all(body).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path)
    };
    let key = write("key.pem", &out.stdout)?;
    let mut config: Vec<String> = text.lines().filter(|l| { let t = l.trim_start(); !t.starts_with("pass_phrase") && !t.starts_with("key_file") }).map(String::from).collect();
    config.push(format!("key_file={}", key.display()));
    let config = write("config", config.join("\n").as_bytes())?;
    Ok(Some((dir, config)))
}

fn oci_region() -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let text = std::fs::read_to_string(Path::new(&home).join(".oci/config")).ok()?;
    text.lines().find_map(|l| l.trim().strip_prefix("region").and_then(|r| r.trim_start().strip_prefix('=')).map(|r| r.trim().to_string())).filter(|r| !r.is_empty())
}

impl Restic {
    pub fn new(password: String) -> Result<Restic, String> {
        let repo = std::env::var("WF_VM_STATE_REPO").ok().filter(|r| !r.is_empty()).unwrap_or_else(|| format!("rclone:oos:{BUCKET}"));
        let mut env = vec![("RESTIC_REPOSITORY".to_string(), repo.clone()), ("RESTIC_PASSWORD".to_string(), password.clone())];
        let mut plain_key = None;
        let mut versions = std::env::var("WF_VM_STATE_VERSIONS_CMD").ok().filter(|c| !c.is_empty()).map(|c| (c, Vec::new()));
        if repo.starts_with("rclone:oos:") {
            let out = run("oci", &["os", "ns", "get"])?;
            let parsed: Value = serde_json::from_slice(&out).map_err(|e| format!("oci os ns get: unparseable output: {e}"))?;
            let namespace = parsed["data"].as_str().filter(|n| !n.is_empty()).ok_or("oci os ns get: no namespace in the answer")?;
            if versions.is_none() {
                versions = Some(("oci".into(), ["os", "object", "list-object-versions", "--namespace-name", namespace, "--bucket-name", BUCKET, "--all", "--fields", "name,size"].map(String::from).to_vec()));
            }
            let region = oci_region().ok_or("~/.oci/config names no region (wf cache guard --local writes it from the vault)")?;
            for (k, v) in [("TYPE", "oracleobjectstorage"), ("PROVIDER", "user_principal_auth"), ("NAMESPACE", namespace), ("REGION", region.as_str()), ("NO_CHECK_BUCKET", "true")] {
                env.push((format!("RCLONE_CONFIG_OOS_{k}"), v.to_string()));
            }
            if let Some((dir, config)) = plain_oci_config()? {
                env.push(("RCLONE_CONFIG_OOS_CONFIG_FILE".to_string(), config.display().to_string()));
                plain_key = Some(dir);
            }
        }
        Ok(Restic { repo, password, env, versions, _plain_key: plain_key })
    }

    pub fn repo(&self) -> &str {
        &self.repo
    }

    pub fn exec(&self, args: &[&str]) -> Result<Out, String> {
        exec("restic", args, &self.env, &self.password)
    }

    /// Stdout of a restic call that must exit 0.
    pub fn run(&self, args: &[&str]) -> Result<Vec<u8>, String> {
        let out = self.exec(args)?;
        if out.code == 0 {
            return Ok(out.stdout);
        }
        Err(format!("restic {} exited {}: {}", args.iter().take(2).copied().collect::<Vec<_>>().join(" "), out.code, out.stderr))
    }

    /// Whether the repository exists. An unreachable or unreadable one is an error, never "no": a restore that mistook it for a
    /// first boot would start an app empty.
    pub fn exists(&self) -> Result<bool, String> {
        let out = self.exec(&["cat", "config"])?;
        match out.code {
            0 => Ok(true),
            10 => Ok(false),
            _ if out.stderr.contains(NO_REPOSITORY) => Ok(false),
            code => Err(format!("restic cat config exited {code}: {}", out.stderr)),
        }
    }

    pub fn init(&self) -> Result<(), String> {
        self.run(&["init", "--repository-version", "2"]).map(|_| ())
    }

    /// Every snapshot, or those of one `host` (the app, the volume and the kind of what a snapshot holds).
    pub fn snapshots(&self, host: Option<&str>) -> Result<Vec<Snapshot>, String> {
        let mut args = vec!["snapshots", "--json"];
        if let Some(host) = host {
            args.extend(["--host", host]);
        }
        let out = self.run(&args)?;
        let parsed: Value = serde_json::from_slice(&out).map_err(|e| format!("restic snapshots: unparseable output: {e}"))?;
        Ok(parsed
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| {
                let strings = |key: &str| s[key].as_array().into_iter().flatten().filter_map(|t| t.as_str().map(String::from)).collect::<Vec<_>>();
                Some(Snapshot { id: s["id"].as_str()?.to_string(), tags: strings("tags"), paths: strings("paths") })
            })
            .collect())
    }

    /// What the bucket is charged for: every version of every object, the previous ones the lifecycle rule has not yet deleted included
    /// (restic's own count sees only the live repository). A repository that is a directory has no versions: its files are all there is.
    pub fn bucket_bytes(&self) -> Result<u64, String> {
        match &self.versions {
            Some((program, args)) => {
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                let out = exec(program, &args, &self.env, &self.password)?;
                if out.code != 0 {
                    return Err(format!("{program} listing the bucket's object versions exited {}: {}", out.code, out.stderr));
                }
                version_bytes(&out.stdout)
            }
            None => Ok(dir_bytes(Path::new(&self.repo))),
        }
    }

    /// Drops snapshots (their packs go with the next prune): a backup that read more than it may removes its own snapshots.
    pub fn forget(&self, ids: &[&str]) -> Result<(), String> {
        let mut args = vec!["forget"];
        args.extend(ids);
        self.run(&args).map(|_| ())
    }

    /// The bytes the repository's objects hold (compressed, deduplicated): what the bucket is charged for.
    pub fn stored_bytes(&self) -> Result<u64, String> {
        let out = self.run(&["stats", "--mode", "raw-data", "--json"])?;
        let parsed: Value = serde_json::from_slice(&out).map_err(|e| format!("restic stats: unparseable output: {e}"))?;
        parsed["total_size"].as_u64().ok_or_else(|| "restic stats: no total_size in the answer".to_string())
    }
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub id: String,
    pub tags: Vec<String>,
    pub paths: Vec<String>,
}

impl Snapshot {
    /// The value of the tag `key:value`.
    pub fn tag(&self, key: &str) -> Option<&str> {
        self.tags.iter().find_map(|t| t.strip_prefix(key).and_then(|r| r.strip_prefix(':')))
    }

    pub fn has(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }

    /// When the backup that wrote it started, from its `ms` tag.
    pub fn created_ms(&self) -> Option<i64> {
        self.tag("ms").and_then(|m| m.parse().ok())
    }
}

/// The newest of `snapshots`: restic's own time is not trusted for ordering, the run stamp is.
pub fn newest(snapshots: impl IntoIterator<Item = Snapshot>) -> Option<Snapshot> {
    snapshots.into_iter().max_by_key(|s| s.created_ms().unwrap_or(0))
}

/// The sizes of an object-version listing (`{"data": [{"size": n, ...}]}`; a delete marker has none) added up.
fn version_bytes(listing: &[u8]) -> Result<u64, String> {
    let parsed: Value = serde_json::from_slice(listing).map_err(|e| format!("the bucket's object versions are unparseable: {e}"))?;
    let rows = parsed["data"].as_array().ok_or("the bucket's object versions have no data list")?;
    Ok(rows.iter().filter_map(|r| r["size"].as_u64()).sum())
}

fn dir_bytes(dir: &Path) -> u64 {
    std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| match e.metadata() {
        Ok(m) if m.is_dir() => dir_bytes(&e.path()),
        Ok(m) => m.len(),
        Err(_) => 0,
    }).sum()
}
