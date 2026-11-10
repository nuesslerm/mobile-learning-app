//! The generated bundle is the one the recorded manifest describes: the filler is no longer committed, so these are what keep it honest.
use orca_map_mobile::testing::Recorded;
use sha2::{Digest, Sha256};

fn hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn given_the_recorded_manifest_when_the_bundle_is_generated_then_every_file_has_the_size_and_hash_the_reference_computed() {
    // Given the manifest the pinned reference built for the recorded files
    let recorded = Recorded::load();
    let manifest = recorded.manifest_result()["manifest"].clone();
    // When the files are generated from their recorded generators
    let dir = recorded.bundle();
    // Then each is the size and sha256 of the asset the reference listed, and the sizes add up to its total
    let mut total = 0;
    for asset in manifest["assets"].as_array().unwrap() {
        let bytes = std::fs::read(dir.path().join(asset["path"].as_str().unwrap())).unwrap();
        assert_eq!(bytes.len() as u64, asset["byteLength"].as_u64().unwrap(), "{asset}");
        assert_eq!(hex(&bytes), asset["sha256"].as_str().unwrap(), "{asset}");
        total += bytes.len() as u64;
    }
    assert_eq!(total, manifest["totalBytes"].as_u64().unwrap());
}

#[test]
fn given_the_recorded_windows_when_the_generated_files_are_cut_then_each_window_has_the_hash_and_length_the_reference_gave() {
    // Given the windows the reference served, one per grid step of each asset
    let recorded = Recorded::load();
    let dir = recorded.bundle();
    let grid = recorded.goldens()["chunkBytes"].as_u64().unwrap() as usize;
    for w in recorded.windows() {
        // When the same window is cut from the generated file
        let bytes = std::fs::read(dir.path().join(w["path"].as_str().unwrap())).unwrap();
        let from = (w["offset"].as_u64().unwrap() as usize).min(bytes.len());
        let piece = &bytes[from..(from + grid).min(bytes.len())];
        // Then its length and hash are the recorded ones
        assert_eq!((piece.len() as u64, hex(piece)), (w["windowLength"].as_u64().unwrap(), w["windowSha256"].as_str().unwrap().to_string()), "{w}");
    }
}

// The set is a recording of the pinned Orca code, and the host's window serving is replayed against it through the wire a phone uses:
// every window of every asset the reference served is what the host serves, and a host with a planted defect is found.
mod replay {
    use serde_json::{json, Value};
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    fn clis() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn executable(args: &[&str]) -> PathBuf {
        let out = Command::new(env!("CARGO")).args(args).arg("--message-format=json").current_dir(clis()).output().unwrap();
        assert!(out.status.success(), "cargo {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).find_map(|m| m["executable"].as_str().map(PathBuf::from)).expect("cargo names the executable")
    }

    fn mimic(args: &[&str], out_dir: Option<&Path>) -> (i32, Value) {
        let bin = std::env::var_os("WF_MIMIC_BIN").map(PathBuf::from).unwrap_or_else(|| executable(&["build", "--locked", "-p", "wf-mimic", "--bin", "wf-mimic"]));
        let mut cmd = Command::new(&bin);
        cmd.arg("mimic").args(args).stdin(Stdio::null());
        if let Some(dir) = out_dir {
            cmd.env("WF_TEST_OUTPUT_DIR", dir);
        }
        let out = cmd.output().unwrap_or_else(|e| panic!("{} could not start: {e}", bin.display()));
        let text = String::from_utf8_lossy(&out.stdout);
        (out.status.code().unwrap_or(-1), serde_json::from_str(&text).unwrap_or_else(|e| panic!("wf-mimic {args:?} printed no JSON ({e}): {text}")))
    }

    /// A target file naming the host's driver, and the same driver with a planted defect, as candidates of the set.
    fn candidates(dir: &Path) -> String {
        let exe = executable(&["build", "-p", "orca-map-mobile", "--example", "mimic_orca_bundle"]).display().to_string();
        let file = dir.join("candidates.toml");
        std::fs::write(&file, format!("[candidate.host]\ncommand = [{exe:?}]\n\n[candidate.defective]\ncommand = [{exe:?}, \"--tweak\"]\n")).unwrap();
        file.display().to_string()
    }

    /// The interactions the host has a counterpart of: those the corpus gives a parity row, which is what a replay proves.
    fn replayed(set: &Path) -> Vec<String> {
        let text = std::fs::read_to_string(set.join("interactions.jsonl")).unwrap();
        text.lines().map(|l| serde_json::from_str::<Value>(l).unwrap()).filter(|i| i.get("row").is_some()).map(|i| i["id"].as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn given_the_recorded_windows_when_the_host_serves_them_then_every_window_is_clean_and_a_defective_host_is_found() {
        // Given the recorded set and the interactions that name a parity row
        let (set, dir) = (orca_map_mobile::testing::set_dir(), tempfile::tempdir().unwrap());
        let (set_arg, only) = (set.display().to_string(), replayed(&set).join(","));
        let target = candidates(dir.path());
        // When the host serves them as suite of a test run
        let (code, clean) = mimic(&["replay", &set_arg, "--target", &target, "--candidate", "host", "--only", &only, "--suite", "wf-mimic-orca-mobile-bundle"], Some(dir.path()));
        // Then each is clean, and the run's trace names the row, the clean diff and that the cassette is a recording of the reference
        assert_eq!((code, clean["severity"].clone()), (0, json!("none")), "{clean}");
        assert_eq!(clean["summary"]["total"].as_u64(), Some(replayed(&set).len() as u64), "{clean}");
        let trace: Value = serde_json::from_str(&std::fs::read_to_string(dir.path().join("wf-mimic-orca-mobile-bundle/trace.json")).unwrap()).unwrap();
        let test = &trace["tests"][0];
        assert_eq!(test["cassette"], "recorded");
        assert!(test["calls"].as_array().unwrap().iter().all(|c| c["diff"] == "clean" && c["rows"].as_array().is_some_and(|r| r.len() == 1)), "{trace}");
        // And a host that forgets the end of an asset is found, which shows the replay reached it
        let (code, bad) = mimic(&["replay", &set_arg, "--target", &target, "--candidate", "defective", "--only", &only], None);
        assert_eq!((code, bad["severity"].clone()), (1, json!("drift")), "{bad}");
    }
}
