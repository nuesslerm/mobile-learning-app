//! `wf vm creds sync` and `wf vm terraform apply|exec`: OCI credentials from the vault, and Terraform with the
//! remote-state credentials injected.
use wf_kit::root::repo_root;
use secrets::{secret, secret_from};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

mod preflight;

const BEGIN: &str = "# --- BEGIN wf deploy sync-creds (managed) ---";
const END: &str = "# --- END wf deploy sync-creds (managed) ---";
const REQUIRED: [&str; 6] = [
    "OCI_TENANCY_OCID",
    "OCI_USER_OCID",
    "OCI_API_KEY_FINGERPRINT",
    "OCI_API_PRIVATE_KEY",
    "OCI_COMPARTMENT_OCID",
    "OCI_REGION",
];
const STATE_REGION: &str = "eu-frankfurt-1";
const A1_FLEX: &str = "VM.Standard.A1.Flex";
const E2_MICRO: &str = "VM.Standard.E2.1.Micro";

/// How a failure prints: a `WfError` as `wf: msg`, anything else as `Error: msg`.
enum Fail {
    Wf(String),
    Plain(String),
}

type Res<T> = Result<T, Fail>;

fn plain<T>(r: Result<T, String>) -> Res<T> {
    r.map_err(Fail::Plain)
}

fn write_private(path: &Path, content: &str) -> Result<(), String> {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| e.to_string())?;
    f.write_all(content.as_bytes()).map_err(|e| e.to_string())
}

fn merge_tfvars(existing: Option<&str>, block: &str) -> String {
    let Some(existing) = existing else {
        return format!("{block}\n\n# Everything else (ssh_public_key_path, sizing overrides,\n# bws_read_access_token, ...) is NOT managed by\n# sync-creds — see terraform.tfvars.example.\n");
    };
    if let (Some(b), Some(e)) = (existing.find(BEGIN), existing.find(END)) {
        if e > b {
            return format!("{}{block}{}", &existing[..b], &existing[e + END.len()..]);
        }
    }
    format!("{}\n\n{block}\n", existing.trim_end())
}

fn sync_creds() -> Res<i32> {
    let mut values: HashMap<&str, String> = HashMap::new();
    let mut missing = Vec::new();
    for key in REQUIRED {
        match plain(secret(key))?.filter(|v| !v.is_empty()) {
            Some(v) => {
                values.insert(key, v);
            }
            None => missing.push(key),
        }
    }
    if !missing.is_empty() {
        return Err(Fail::Wf(format!(
            "Missing OCI credentials in the vault: {}. Run `wf secret set <KEY>` for each; `wf secret list` says where each value comes from.",
            missing.join(", ")
        )));
    }
    let optional = |key: &str| plain(secret(key)).map(|v| v.filter(|s| !s.is_empty()));

    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let pem_dir = home.join(".oci");
    let pem_path = pem_dir.join("oci_api_key.pem");
    let io = |r: std::io::Result<()>| plain(r.map_err(|e| e.to_string()));
    {
        use std::os::unix::fs::DirBuilderExt;
        io(fs::DirBuilder::new().recursive(true).mode(0o700).create(&pem_dir))?;
    }
    plain(write_private(
        &pem_path,
        &format!("{}\n", values["OCI_API_PRIVATE_KEY"].trim()),
    ))?;

    let tfvars = repo_root()
        .join("infra")
        .join("oracle-cloud")
        .join("terraform.tfvars");
    let existing = fs::read_to_string(&tfvars).ok();
    let passphrase = optional("OCI_API_PRIVATE_KEY_PASSPHRASE")?.unwrap_or_default();
    let mut lines = vec![
        BEGIN.to_string(),
        format!("tenancy_ocid = \"{}\"", values["OCI_TENANCY_OCID"]),
        format!("user_ocid = \"{}\"", values["OCI_USER_OCID"]),
        format!("api_key_fingerprint = \"{}\"", values["OCI_API_KEY_FINGERPRINT"]),
        format!("api_private_key_path = \"{}\"", pem_path.display()),
        format!("api_private_key_passphrase = \"{passphrase}\""),
        format!("region = \"{}\"", values["OCI_REGION"]),
        format!("compartment_ocid = \"{}\"", values["OCI_COMPARTMENT_OCID"]),
    ];
    if let Some(k) = optional("PAGERDUTY_INTEGRATION_KEY")? {
        lines.push(format!("pagerduty_integration_key = \"{k}\""));
    }
    if let Some(d) = optional("PAGERDUTY_EVENTS_DOMAIN")? {
        lines.push(format!("pagerduty_events_domain = \"{d}\""));
    }
    if let Some(e) = optional("BUDGET_ALERT_EMAIL")? {
        lines.push(format!("budget_alert_email = \"{e}\""));
    }
    lines.push(END.to_string());
    io(fs::write(&tfvars, merge_tfvars(existing.as_deref(), &lines.join("\n"))))?;

    let mut cli = vec![
        "[DEFAULT]".to_string(),
        format!("user={}", values["OCI_USER_OCID"]),
        format!("fingerprint={}", values["OCI_API_KEY_FINGERPRINT"]),
        format!("tenancy={}", values["OCI_TENANCY_OCID"]),
        format!("region={}", values["OCI_REGION"]),
        format!("key_file={}", pem_path.display()),
    ];
    if !passphrase.is_empty() {
        cli.push(format!("pass_phrase={passphrase}"));
    }
    plain(write_private(&pem_dir.join("config"), &format!("{}\n", cli.join("\n"))))?;
    Ok(0)
}

/// The state-bucket credentials, always from the bitwarden backend (they are admin-scoped).
fn backend_env() -> Res<Vec<(String, String)>> {
    let id = plain(secret_from("OCI_TFSTATE_S3_ACCESS_KEY_ID", "bitwarden"))?.filter(|v| !v.is_empty());
    let key =
        plain(secret_from("OCI_TFSTATE_S3_SECRET_ACCESS_KEY", "bitwarden"))?.filter(|v| !v.is_empty());
    let (Some(id), Some(key)) = (id.clone(), key) else {
        return Err(Fail::Plain(missing_message(id.is_some())));
    };
    Ok(vec![
        ("AWS_ACCESS_KEY_ID".into(), id),
        ("AWS_SECRET_ACCESS_KEY".into(), key),
        ("AWS_REGION".into(), STATE_REGION.into()),
        ("AWS_REQUEST_CHECKSUM_CALCULATION".into(), "when_required".into()),
        ("AWS_RESPONSE_CHECKSUM_VALIDATION".into(), "when_required".into()),
    ])
}

fn missing_message(id_present: bool) -> String {
    let missing = if id_present {
        "OCI_TFSTATE_S3_SECRET_ACCESS_KEY"
    } else {
        "OCI_TFSTATE_S3_ACCESS_KEY_ID and OCI_TFSTATE_S3_SECRET_ACCESS_KEY"
    };
    format!(
        "Cannot reach Terraform remote state: {missing} did not resolve from the vault. These are the OCI Customer Secret Key halves the S3-compatible backend authenticates with (infra/oracle-cloud/versions.tf). Check `wf secret doctor --backend bitwarden`; if they are genuinely absent, mint a new Customer Secret Key (OCI Console -> Identity -> Users -> your user) and store both halves with `wf secret create`."
    )
}

struct Out {
    ok: bool,
    stdout: String,
    stderr: String,
}

struct Change {
    address: String,
    actions: Vec<String>,
    shape: Option<String>,
    ocpus: Option<f64>,
    memory: Option<f64>,
    /// The A1 pool this resource holds today, freed when the change applies.
    held: (f64, f64),
}

fn number(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::Null) => 0.0,
        Some(Value::String(s)) => s.trim().parse().unwrap_or(f64::NAN),
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        _ => f64::NAN,
    }
}

fn scratch(prefix: &str) -> Res<PathBuf> {
    let dir = std::env::temp_dir().join(format!("{prefix}{}", std::process::id()));
    plain(fs::create_dir_all(&dir).map_err(|e| e.to_string()))?;
    Ok(dir)
}

fn plan(dir: &str, extra: &[&str], vars: &[(String, String)]) -> Res<Vec<Change>> {
    let tmp = scratch("wf-tf-plan-")?;
    let file = tmp.join("plan.tfplan");
    let result = (|| {
        let out = format!("-out={}", file.display());
        let planned = terraform_with(dir, &[&["plan", "-input=false"], extra, &[out.as_str()]].concat(), vars)?;
        if !planned.ok {
            return Err(Fail::Plain(format!("terraform plan failed: {}", planned.stderr)));
        }
        let shown = terraform_with(dir, &["show", "-json", &file.display().to_string()], vars)?;
        if !shown.ok {
            return Err(Fail::Plain(format!("terraform show -json failed: {}", shown.stderr)));
        }
        let parsed: Value = plain(serde_json::from_str(&shown.stdout).map_err(|e| e.to_string()))?;
        Ok(parsed["resource_changes"]
            .as_array()
            .map(|a| a.as_slice())
            .unwrap_or_default()
            .iter()
            .map(|c| {
                let after = &c["change"]["after"];
                let cfg = after["shape_config"].as_array().and_then(|a| a.first());
                let before = &c["change"]["before"];
                let held = before["shape_config"].as_array().and_then(|a| a.first()).filter(|_| before["shape"] == A1_FLEX);
                Change {
                    address: c["address"].as_str().unwrap_or_default().to_string(),
                    actions: c["change"]["actions"]
                        .as_array()
                        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                        .unwrap_or_default(),
                    shape: after["shape"].as_str().map(String::from),
                    ocpus: cfg.map(|c| number(c.get("ocpus"))),
                    memory: cfg.map(|c| number(c.get("memory_in_gbs"))),
                    held: held.map_or((0.0, 0.0), |c| (number(c.get("ocpus")), number(c.get("memory_in_gbs")))),
                }
            })
            .collect())
    })();
    let _ = fs::remove_dir_all(&tmp);
    result
}

fn n(v: f64) -> String {
    if v.is_nan() {
        "NaN".into()
    } else if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        v.to_string()
    }
}

/// `None` when the change fits the Always Free tier, else the violation.
fn free_tier(shape: &str, ocpus: f64, memory: f64, used: (f64, f64)) -> Option<String> {
    if shape == A1_FLEX {
        let (o, m) = (used.0 + ocpus, used.1 + memory);
        // Oracle halved the Always-Free A1 pool to 2 OCPU / 12 GB on 2026-06-15 (infra/oracle-cloud/always-free-quota.tf)
        if o > 2.0 {
            return Some(format!("VM.Standard.A1.Flex OCPU pool would be {}, exceeding the free-tier limit of 2", n(o)));
        }
        if m > 12.0 {
            return Some(format!("VM.Standard.A1.Flex memory pool would be {}GB, exceeding the free-tier limit of 12GB", n(m)));
        }
        return None;
    }
    if shape == E2_MICRO {
        if ocpus > 1.0 || memory > 1.0 {
            return Some(format!("VM.Standard.E2.1.Micro requested with {} OCPU/{}GB, exceeding its fixed free spec", n(ocpus), n(memory)));
        }
        return None;
    }
    Some(format!("Shape \"{shape}\" is not a recognized Always Free shape -- refusing to avoid unexpected billing"))
}

fn a1_usage(compartment: &str) -> Res<(f64, f64)> {
    let args = ["compute", "instance", "list", "--compartment-id", compartment, "--all"];
    let out = Command::new("oci").args(args).stdin(Stdio::null()).output();
    let label = "oci compute instance list";
    let o = out.map_err(|e| Fail::Plain(format!("{label} failed: {e}")))?;
    if !o.status.success() {
        let stderr = String::from_utf8_lossy(&o.stderr).trim().to_string();
        let detail = if stderr.is_empty() { format!("Command failed: oci {}", args.join(" ")) } else { stderr };
        return Err(Fail::Plain(format!("{label} failed: {detail}")));
    }
    let text = String::from_utf8_lossy(&o.stdout).into_owned();
    let parsed: Value = serde_json::from_str(&text).map_err(|_| {
        Fail::Plain(format!("{label} returned unparseable JSON: {}", text.chars().take(200).collect::<String>()))
    })?;
    let mut used = (0.0, 0.0);
    for d in parsed["data"].as_array().map(|a| a.as_slice()).unwrap_or_default() {
        if d["shape"] == A1_FLEX && d["lifecycle-state"] != "TERMINATED" {
            if let Some(cfg) = d.get("shape-config").filter(|c| !c.is_null()) {
                used.0 += number(cfg.get("ocpus"));
                used.1 += number(cfg.get("memory-in-gbs"));
            }
        }
    }
    Ok(used)
}

/// The first create or update that would leave the free tier; the pool those changes hold today counts as freed.
fn guard(changes: &[Change], compartment: &str) -> Res<Option<String>> {
    let (o, m) = a1_usage(compartment)?;
    let sized: Vec<&Change> = changes.iter().filter(|c| c.actions.iter().any(|a| a == "create" || a == "update")).collect();
    let used = sized.iter().fold((o, m), |(o, m), c| (o - c.held.0, m - c.held.1));
    for c in sized {
        let Some(shape) = &c.shape else { continue };
        if let Some(reason) = free_tier(shape, c.ocpus.unwrap_or(0.0), c.memory.unwrap_or(0.0), used) {
            return Ok(Some(format!("free-tier guard blocked {}: {reason}", c.address)));
        }
    }
    Ok(None)
}

const RETIRE_ATTEMPTS: u32 = 4;

/// Deletes the destroyed VM's tailnet records, retrying with a growing pause. A record left behind makes the new nodes register as
/// `<name>-1`, which vm-gateway cannot reach, so a failure here is an error that stops before the create (`None` means go on).
fn retire_or_stop(dir: &str, compartment: &str, which: &str) -> Option<i32> {
    let pause = std::env::var("WF_TAILNET_RETIRE_BACKOFF_MS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(5000);
    let mut errors = Vec::new();
    for attempt in 1..=RETIRE_ATTEMPTS {
        match crate::tailscale::retire_vm_records() {
            Ok(gone) => {
                wf_kit::wf_err!("wf: retired {} tailnet record(s) of the {which} VM: {}", gone.len(), gone.join(", "));
                return None;
            }
            Err(e) => {
                wf_kit::wf_err!("wf: tailnet cleanup attempt {attempt}/{RETIRE_ATTEMPTS} failed: {e}");
                errors.push(json!({ "attempt": attempt, "error": e }));
                if attempt < RETIRE_ATTEMPTS {
                    std::thread::sleep(std::time::Duration::from_millis(pause * u64::from(attempt)));
                }
            }
        }
    }
    println!(
        "{}",
        json!({
            "ok": false,
            "reason": format!("the {which} VM's tailnet records could not be deleted after {RETIRE_ATTEMPTS} attempts; the VM is destroyed and the create was not started, because the new nodes would register as <name>-1"),
            "vmDestroyed": true,
            "created": false,
            "rolledBack": false,
            "evidence": errors,
            "next": ["wf vm net device retire --replaced-vm --apply", format!("wf vm terraform apply {dir} {compartment}")],
        })
    );
    Some(1)
}

fn apply(dir: &str, compartment: &str, vars: &[(String, String)]) -> Res<i32> {
    let changes = plan(dir, &[], vars)?;
    if let Some(reason) = guard(&changes, compartment)? {
        println!("{}", json!({ "ok": false, "reason": reason, "rolledBack": false }));
        return Ok(1);
    }
    let pulled = terraform_with(dir, &["state", "pull"], vars)?;
    if !pulled.ok {
        return Err(Fail::Plain(format!("terraform state pull failed: {}", pulled.stderr)));
    }
    if changes.iter().any(|c| {
        c.address == "oci_core_instance.main" && c.actions.iter().any(|a| a == "delete") && c.actions.iter().any(|a| a == "create")
    }) {
        // A replace is destroy-then-create in one apply, which leaves no gap to clear the old node's tailnet records.
        let destroyed = terraform_with(dir, &["destroy", "-target=oci_core_instance.main", "-auto-approve", "-input=false"], vars)?;
        if !destroyed.ok {
            println!("{}", json!({ "ok": false, "reason": "terraform destroy of the VM failed", "rolledBack": false, "stdout": destroyed.stdout, "stderr": destroyed.stderr }));
            return Ok(1);
        }
        if let Some(code) = retire_or_stop(dir, compartment, "replaced") {
            return Ok(code);
        }
    }
    let mut applied = terraform_with(dir, &["apply", "-auto-approve", "-input=false"], vars)?;
    for _ in 0..10 {
        if applied.ok || !applied.stderr.contains("QuotaExceeded") {
            break;
        }
        wf_kit::wf_err!("wf: OCI still counts a just-destroyed instance's quota; retrying the apply in 60 s");
        std::thread::sleep(std::time::Duration::from_secs(60));
        applied = terraform_with(dir, &["apply", "-auto-approve", "-input=false"], vars)?;
    }
    if !applied.ok {
        println!("{}", json!({ "ok": false, "reason": "terraform apply failed", "rolledBack": false, "stdout": applied.stdout, "stderr": applied.stderr }));
        return Ok(1);
    }
    let pending = plan(dir, &[], vars)?
        .iter()
        .filter(|c| !c.actions.iter().all(|a| a == "no-op"))
        .count();
    if pending > 0 {
        let tmp = scratch("wf-tf-state-")?;
        let file = tmp.join("restore.tfstate");
        plain(fs::write(&file, &pulled.stdout).map_err(|e| e.to_string()))?;
        let pushed = terraform_with(dir, &["state", "push", "-force", &file.display().to_string()], vars);
        let _ = fs::remove_dir_all(&tmp);
        let pushed = pushed?;
        if !pushed.ok {
            return Err(Fail::Plain(format!("terraform state push failed: {}", pushed.stderr)));
        }
        println!("{}", json!({ "ok": false, "reason": format!("post-apply plan still shows {pending} pending change(s) -- rolled back to the pre-apply state"), "rolledBack": true, "stdout": applied.stdout, "stderr": applied.stderr }));
        return Ok(1);
    }
    println!("{}", json!({ "ok": true, "stdout": applied.stdout }));
    Ok(0)
}

/// The inputs a VM create needs that `terraform.tfvars` does not hold: the vault bootstrap token (a VM without it serves
/// nothing), the ADB admin password, and the deploy key's public half (made once when missing). An env value wins.
fn vm_vars() -> Res<Vec<(String, String)>> {
    let mut vars = Vec::new();
    for (var, key) in [("TF_VAR_bws_read_access_token", "BWS_ADMIN_ACCESS_TOKEN"), ("TF_VAR_adb_admin_password", "ADB_ADMIN_PASSWORD")] {
        if std::env::var(var).is_ok_and(|v| !v.is_empty()) {
            continue;
        }
        let value = std::env::var(key).ok().filter(|v| !v.is_empty()).or(plain(secret_from(key, "bitwarden"))?.filter(|v| !v.is_empty()));
        let value = value.ok_or_else(|| Fail::Plain(format!("{key} is in neither the environment nor the vault; a VM created without it serves nothing")))?;
        vars.push((var.to_string(), value));
    }
    if std::env::var("TF_VAR_ssh_public_key_path").is_err() {
        let key = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/root".into())).join(".ssh/oracle_deploy");
        if !key.with_extension("pub").is_file() {
            let _ = fs::create_dir_all(key.parent().unwrap_or(Path::new(".")));
            let made = Command::new("ssh-keygen").args(["-q", "-t", "ed25519", "-N", "", "-C", "oracle-deploy", "-f"]).arg(&key).status();
            if !made.is_ok_and(|s| s.success()) {
                return Err(Fail::Plain("ssh-keygen could not make the deploy key".into()));
            }
        }
        vars.push(("TF_VAR_ssh_public_key_path".into(), key.with_extension("pub").display().to_string()));
    }
    Ok(vars)
}

fn terraform_with(dir: &str, args: &[&str], vars: &[(String, String)]) -> Res<Out> {
    let out = Command::new("terraform").args(args).current_dir(dir).envs(backend_env()?).envs(vars.iter().cloned()).stdin(Stdio::null()).output();
    Ok(match out {
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr).trim().to_string();
            let ok = o.status.success();
            let stderr = if ok || !stderr.is_empty() { stderr } else { format!("Command failed: terraform {}", args.join(" ")) };
            Out { ok, stdout: String::from_utf8_lossy(&o.stdout).into_owned(), stderr }
        }
        Err(e) => Out { ok: false, stdout: String::new(), stderr: e.to_string() },
    })
}

/// `wf vm terraform apply --recreate [dir]`: the VM from clean. Destroys `oci_core_instance.main`, deletes every tailnet
/// record its nodes left (so the new ones claim the bare names vm-gateway and CI dial, not `-1`), then runs the guarded
/// `apply`, which creates it again; cloud-init provisions every app in `vm_apps`.
fn recreate(dir: &str) -> Res<i32> {
    let vars = vm_vars()?;
    let compartment = plain(secret("OCI_COMPARTMENT_OCID"))?.filter(|v| !v.is_empty()).ok_or_else(|| Fail::Plain("OCI_COMPARTMENT_OCID is in neither the environment nor the vault".into()))?;
    let state = terraform_with(dir, &["state", "list"], &vars)?;
    if !state.stdout.lines().any(|l| l.trim() == "oci_core_instance.main") {
        println!("{}", json!({ "ok": false, "reason": "the remote state does not hold oci_core_instance.main: destroying would strand the recreate", "stderr": state.stderr }));
        return Ok(1);
    }
    let destroyed = terraform_with(dir, &["destroy", "-target=oci_core_instance.main", "-auto-approve", "-input=false", "-no-color"], &vars)?;
    if !destroyed.ok {
        println!("{}", json!({ "ok": false, "reason": "terraform destroy of the VM failed", "stderr": destroyed.stderr }));
        return Ok(1);
    }
    if let Some(code) = retire_or_stop(dir, &compartment, "old") {
        return Ok(code);
    }
    apply(dir, &compartment, &vars)
}

fn exec(dir: &str, args: &[String]) -> Res<i32> {
    if args.is_empty() {
        return Ok(2);
    }
    let env = backend_env()?;
    // the VM inputs too, so a raw plan or import sees the same variables as the guarded apply
    let vars = vm_vars().unwrap_or_default();
    let status = Command::new("terraform").args(args).current_dir(dir).envs(env).envs(vars).stdin(Stdio::inherit()).status();
    Ok(match status {
        Ok(s) => s.code().unwrap_or(1),
        Err(_) => 1,
    })
}

fn finish(r: Res<i32>) -> Option<i32> {
    Some(match r {
        Ok(code) => code,
        Err(Fail::Wf(m)) => {
            wf_kit::wf_err!("wf: {m}");
            1
        }
        Err(Fail::Plain(m)) => {
            eprintln!("Error: {m}");
            1
        }
    })
}

/// `wf vm creds sync`; any extra argument is clap's error.
pub fn run_sync_creds(args: &[String]) -> Option<i32> {
    wf_kit::args::parse_none(args, &[], &[])?;
    finish(sync_creds())
}

/// `wf vm terraform <apply|preflight|exec> ...`. `exec` hands everything after the directory to Terraform untouched, `wf`'s own options
/// included.
pub fn run_tf(args: &[String]) -> Option<i32> {
    use wf_kit::args::{parse_spec, Positional, Spec};
    let (sub, rest) = args.split_first()?;
    match sub.as_str() {
        "apply" => {
            let p = parse_spec(rest, &Spec { positional: Positional::AtMost(2), flags: &["recreate"], ..Spec::NONE })?;
            if p.flag("recreate") {
                let dir = p.positional.first().cloned().unwrap_or_else(|| repo_root().join("infra/oracle-cloud").display().to_string());
                return finish(recreate(&dir));
            }
            (p.positional.len() == 2).then(|| finish(vm_vars().and_then(|vars| apply(&p.positional[0], &p.positional[1], &vars)))).flatten()
        }
        "preflight" => {
            let p = parse_spec(rest, &Spec { values: &["expect"], positional: Positional::AtMost(1), ..Spec::NONE })?;
            let dir = p.positional.first().cloned().unwrap_or_else(|| repo_root().join("infra/oracle-cloud").display().to_string());
            let expect: Vec<String> = p.values("expect").into_iter().map(String::from).collect();
            finish(preflight::run(&dir, &expect))
        }
        "exec" => {
            let dir = parse_spec(rest.get(..1)?, &Spec { positional: Positional::Exactly(1), ..Spec::NONE })?;
            // a raw replace of the VM leaves its tailnet records holding the bare names, so the new nodes come up as `-1`
            if rest[1..].iter().any(|a| a.contains("-replace") && a.contains("oci_core_instance.main")) {
                println!("{}", json!({ "error": "wf vm terraform exec does not replace the VM: a raw -replace leaves the old tailnet records holding the bare hostnames", "next": "wf vm terraform apply --recreate" }));
                return Some(2);
            }
            finish(exec(&dir.positional[0], &rest[1..]))
        }
        _ => None,
    }
}
