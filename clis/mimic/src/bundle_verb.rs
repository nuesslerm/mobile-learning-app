//! `wf mimic publish` and `wf mimic fetch`: the transport of a recorded set from the VM runner to wherever it is committed. `publish` uploads the
//! masked files of sets (never `raw/`) to the verification-artifacts bucket under `verification/record-references/<run>/`, manifest last, and
//! `fetch` reads them back over the bucket's public URL with every hash checked and writes them where they belong in a checkout. The
//! transport is plain HTTPS to the bucket, so it works from an agent session whose proxy refuses the GitHub Actions API.
use super::{fail, set_ref, Done, Fail};
use mimic_core::json::sha256_label;
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use wf_bucket::artifacts::{content_type_for, par_url, public_base, put_par, read_public};
use wf_kit::args::Parsed;

const PREFIX: &str = "verification/record-references";
const KEEP: [&str; 5] = ["json", "jsonl", "toml", "txt", "md"];
const FILE_CAP: u64 = 60 * 1024 * 1024;
const BUNDLE_CAP: u64 = 200 * 1024 * 1024;

fn plain(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')) && s != "." && s != ".."
}

fn safe_rel(rel: &str) -> bool {
    !rel.is_empty() && !rel.starts_with('/') && rel.split('/').all(|seg| !seg.is_empty() && seg != "." && seg != "..")
}

fn files_of(dir: &Path, base: &Path, out: &mut Vec<String>, skipped: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let (path, name) = (e.path(), e.file_name().to_string_lossy().into_owned());
        let rel = path.strip_prefix(base).unwrap_or(&path).to_string_lossy().into_owned();
        if name.starts_with('.') || name == "raw" {
            continue;
        }
        if path.is_dir() {
            files_of(&path, base, out, skipped);
        } else if path.extension().and_then(|x| x.to_str()).is_some_and(|x| KEEP.contains(&x)) {
            out.push(rel);
        } else {
            skipped.push(rel);
        }
    }
}

pub fn publish_verb(p: &Parsed) -> Done {
    if p.positional.is_empty() {
        return Err(fail("name at least one set", "wf mimic publish <set>... --run <id>"));
    }
    let run = p.val("run").map(String::from).or_else(|| std::env::var("GITHUB_RUN_ID").ok().filter(|v| !v.is_empty())).unwrap_or_else(|| "local".into());
    if !plain(&run) {
        return Err(fail(format!("--run {run:?} is not a plain id (letters, digits, . _ -)"), "wf mimic publish --help"));
    }
    let root = p.val("root").map_or_else(wf_kit::root::repo_root, PathBuf::from);
    let par = par_url().map_err(|e| fail(e, "the runner reads VERIFICATION_ARTIFACTS_PAR_URL from the vault (wf secret get); locally export it first"))?;
    let (mut sets, mut uploads, mut total, mut warnings) = (Vec::<Value>::new(), Vec::<(String, Vec<u8>)>::new(), 0u64, Vec::<String>::new());
    for arg in &p.positional {
        let set = set_ref(arg);
        if !plain(&set.name) {
            return Err(fail(format!("set name {:?} is not a plain directory name", set.name), "rename the set directory"));
        }
        if !set.dir.join("cassette.json").is_file() {
            return Err(fail(format!("{} has no cassette.json: only a recorded set is published", set.dir.display()), "wf mimic record <set> first"));
        }
        let (mut rels, mut skipped) = (Vec::new(), Vec::new());
        files_of(&set.dir, &set.dir, &mut rels, &mut skipped);
        warnings.extend(skipped.iter().map(|s| format!("{}/{s} is not a kept file type and was left out", set.name)));
        let mut files = Vec::new();
        for rel in rels {
            let bytes = std::fs::read(set.dir.join(&rel)).map_err(|e| fail(format!("{}/{rel}: {e}", set.name), "check the set is readable"))?;
            if bytes.len() as u64 > FILE_CAP || total + bytes.len() as u64 > BUNDLE_CAP {
                return Err(fail(format!("{}/{rel} ({} bytes) is over the bundle caps ({FILE_CAP} a file, {BUNDLE_CAP} in all)", set.name, bytes.len()), "shrink the corpus or record the set in parts"));
            }
            total += bytes.len() as u64;
            files.push(json!({"path": rel, "bytes": bytes.len(), "sha256": sha256_label(&bytes)}));
            uploads.push((format!("{PREFIX}/{run}/{}/{rel}", set.name), bytes));
        }
        sets.push(json!({"name": set.name, "dir": crate::config::relative(&root, &set.dir), "files": files}));
    }
    for (key, body) in &uploads {
        put_par(&format!("{par}{key}"), body, content_type_for(key)).map_err(|e| fail(format!("upload of {key}: {e}"), "check VERIFICATION_ARTIFACTS_PAR_URL and the network, then publish again"))?;
    }
    let manifest = json!({"schema": "mimic-bundle/1", "run": run, "created": wf_kit::time::iso(wf_kit::time::now_ms()), "sets": sets});
    put_par(&format!("{par}{PREFIX}/{run}/manifest.json"), serde_json::to_string_pretty(&manifest).unwrap_or_default().as_bytes(), "application/json").map_err(|e| fail(format!("upload of the manifest: {e}"), "publish again"))?;
    for s in &sets {
        let latest = format!("{PREFIX}/latest/{}.json", s["name"].as_str().unwrap_or_default());
        put_par(&format!("{par}{latest}"), json!({"run": run}).to_string().as_bytes(), "application/json").map_err(|e| fail(format!("upload of {latest}: {e}"), "publish again"))?;
    }
    let names: Vec<&str> = sets.iter().filter_map(|s| s["name"].as_str()).collect();
    let fetch = format!("wf mimic fetch {run}");
    if let Some(path) = p.val("summary").map(PathBuf::from).or_else(|| std::env::var_os("GITHUB_STEP_SUMMARY").map(PathBuf::from)) {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let dirs: Vec<&str> = sets.iter().filter_map(|s| s["dir"].as_str()).collect();
            let _ = write!(f, "Published {} file(s), {total} bytes, as run `{run}`. In a checkout of `dev`:\n\n```sh\n{fetch}\nwf mimic verify {}\ngit add {} && git commit -m \"mimic: re-record {}\"\n```\n\nThe same files are in the workflow artifact `{}`.\n\n", uploads.len(), dirs.join(" "), dirs.join(" "), names.join(" "), names.join("`, `"));
        }
    }
    Ok((json!({"ok": true, "run": run, "sets": sets, "files": uploads.len(), "bytes": total, "fetch": fetch, "warnings": warnings, "next": fetch}), 0))
}

fn get(url: &str) -> Result<String, Fail> {
    read_public(url).map_err(|e| fail(e, "check the run id (wf mimic fetch latest --set <name> finds the newest)"))?.ok_or_else(|| fail(format!("{url} does not exist"), "check the run id: it is the id the workflow summary printed"))
}

pub fn fetch_verb(p: &Parsed) -> Done {
    let [target] = p.positional.as_slice() else { return Err(fail("name one run id, or `latest` with --set", "wf mimic fetch <run-id>")) };
    let root = p.val("root").map_or_else(wf_kit::root::repo_root, PathBuf::from);
    let base = format!("{}/{PREFIX}", public_base());
    let run = if target == "latest" {
        let set = p.val("set").ok_or_else(|| fail("`latest` needs --set <name>", "wf mimic fetch latest --set <name>"))?;
        let pointer: Value = serde_json::from_str(&get(&format!("{base}/latest/{set}.json"))?).map_err(|e| fail(format!("latest/{set}.json: {e}"), "publish the set again"))?;
        pointer["run"].as_str().map(String::from).ok_or_else(|| fail(format!("latest/{set}.json names no run"), "publish the set again"))?
    } else {
        target.clone()
    };
    if !plain(&run) {
        return Err(fail(format!("run id {run:?} is not a plain id"), "wf mimic fetch <run-id>"));
    }
    let manifest: Value = serde_json::from_str(&get(&format!("{base}/{run}/manifest.json"))?).map_err(|e| fail(format!("manifest of run {run}: {e}"), "publish the run again"))?;
    if manifest["schema"] != json!("mimic-bundle/1") {
        return Err(fail(format!("manifest of run {run} has schema {}, not mimic-bundle/1", manifest["schema"]), "update wf"));
    }
    let wanted = p.val("set");
    let mut done = Vec::<Value>::new();
    for s in manifest["sets"].as_array().into_iter().flatten().filter(|s| wanted.is_none_or(|w| s["name"] == json!(w))) {
        let (name, dir) = (s["name"].as_str().unwrap_or_default(), s["dir"].as_str().unwrap_or_default());
        if !plain(name) || !safe_rel(dir) {
            return Err(fail(format!("manifest names set {name:?} at {dir:?}: not a plain name and a path inside the checkout"), "do not trust this bundle"));
        }
        let mut n = 0;
        for f in s["files"].as_array().into_iter().flatten() {
            let rel = f["path"].as_str().unwrap_or_default();
            if !safe_rel(rel) {
                return Err(fail(format!("manifest names file {rel:?}: not a path inside the set"), "do not trust this bundle"));
            }
            let body = get(&format!("{base}/{run}/{name}/{rel}"))?;
            if f["sha256"] != json!(sha256_label(body.as_bytes())) {
                return Err(fail(format!("{name}/{rel} does not match the hash in the manifest: nothing more was written"), "fetch again; if it repeats, publish the run again"));
            }
            let dest = root.join(dir).join(rel);
            std::fs::create_dir_all(dest.parent().unwrap_or(&root)).and_then(|()| std::fs::write(&dest, body)).map_err(|e| fail(format!("{}: {e}", dest.display()), "check the checkout is writable"))?;
            n += 1;
        }
        done.push(json!({"name": name, "dir": dir, "files": n}));
    }
    if done.is_empty() {
        return Err(fail(format!("run {run} has no set {}", wanted.unwrap_or("(any)")), "wf mimic fetch <run-id> without --set lists what it has"));
    }
    let dirs: Vec<&str> = done.iter().filter_map(|d| d["dir"].as_str()).collect();
    Ok((json!({"ok": true, "run": run, "sets": done, "warnings": [], "next": format!("wf mimic verify {} && git add {}", dirs.join(" "), dirs.join(" "))}), 0))
}
