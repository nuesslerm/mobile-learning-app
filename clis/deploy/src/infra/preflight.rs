//! `wf vm terraform preflight [dir]`: everything that must hold before `wf vm terraform apply --recreate`, in one call with one JSON
//! verdict: the remote state is backed up and holds the VM, the Terraform user may create the tenancy-scoped dynamic group the restore needs, every secret a recreate reads is present and usable, every app's state has a fresh verified backup, no CI run is in
//! flight, and the plan of the recreate destroys exactly the resources it is expected to. It changes nothing but the local backup.
use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

/// What a recreate may destroy: the VM itself and the PAR whose replacement is the known drift. Anything else destroyed fails the plan check.
const EXPECTED_DESTROYS: [&str; 2] = ["oci_core_instance.main", "oci_objectstorage_preauthrequest.libs_read"];
const VM: &str = "oci_core_instance.main";
/// A replaced resource whose new value is minted by Terraform and lives in the vault, with the output and the vault key that carry it over.
const MINTED: [(&str, &str, &str); 1] = [("oci_objectstorage_preauthrequest.libs_read", "libs_read_par_url", "LIBS_READ_PAR_URL")];
/// Secrets a recreate reads beyond each app's own group: the Terraform inputs.
const TERRAFORM_EXTRAS: [&str; 6] = ["BWS_DEPLOY_SECRETS_READ_ACCESS_TOKEN", "ADB_ADMIN_PASSWORD", "OCI_TFSTATE_S3_ACCESS_KEY_ID", "OCI_TFSTATE_S3_SECRET_ACCESS_KEY", "TS_VM_OAUTH_CLIENT_SECRET", "TS_TAILNET_ID"];
const IN_FLIGHT: [&str; 4] = ["in_progress", "queued", "waiting", "pending"];

fn check(name: &str, ok: bool, detail: String, next: &str) -> Value {
    let mut c = json!({ "name": name, "ok": ok, "detail": detail });
    if !ok {
        c["next"] = json!(next);
    }
    c
}

fn message(f: Fail) -> String {
    match f {
        Fail::Wf(m) | Fail::Plain(m) => m,
    }
}

fn backup_dir() -> PathBuf {
    match std::env::var("WF_TFSTATE_BACKUP_DIR").ok().filter(|d| !d.is_empty()) {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/root".into())).join(".local/state/wf/tfstate-backups"),
    }
}

/// Pulls the remote state, keeps a copy that matches its serial (an existing identical one is the backup; else a new file), and proves it holds the VM.
fn state(dir: &str, vars: &[(String, String)], pulled_state: &mut Value) -> Res<Value> {
    let pulled = terraform_with(dir, &["state", "pull"], vars)?;
    if !pulled.ok {
        return Ok(check("tfstate", false, format!("terraform state pull failed: {}", pulled.stderr), "check the OCI_TFSTATE_S3_* secrets and that the state bucket is reachable"));
    }
    let parsed: Value = match serde_json::from_str(&pulled.stdout) {
        Ok(v) => v,
        Err(e) => return Ok(check("tfstate", false, format!("the remote state is not JSON ({e}); it is empty or unreadable"), "restore the state from the bucket's object versions before destroying anything")),
    };
    let serial = parsed["serial"].as_u64().unwrap_or(0);
    let holds_vm = parsed["resources"].as_array().is_some_and(|r| r.iter().any(|x| x["type"] == "oci_core_instance" && x["name"] == "main"));
    if !holds_vm {
        return Ok(check("tfstate", false, format!("the remote state (serial {serial}) does not hold {VM}: destroying would strand the recreate"), "import or restore the state first"));
    }
    *pulled_state = parsed.clone();
    let backups = backup_dir();
    plain(fs::create_dir_all(&backups).map_err(|e| e.to_string()))?;
    let suffix = format!("-serial{serial}.json");
    let existing = fs::read_dir(&backups).ok().into_iter().flatten().flatten().map(|e| e.path()).find(|p| {
        p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("tfstate-") && n.ends_with(&suffix)) && fs::read_to_string(p).is_ok_and(|t| t == pulled.stdout)
    });
    let (path, written) = match existing {
        Some(p) => (p, false),
        None => {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
            let p = backups.join(format!("tfstate-{now}{suffix}"));
            plain(write_private(&p, &pulled.stdout))?;
            (p, true)
        }
    };
    if !fs::read_to_string(&path).is_ok_and(|t| t == pulled.stdout) {
        return Ok(check("tfstate", false, format!("the backup {} does not read back equal to the remote state", path.display()), "free disk space and rerun"));
    }
    Ok(check("tfstate", true, format!("serial {serial}, backup {} ({})", path.display(), if written { "written now" } else { "already current" }), ""))
}

fn state_holds(state: &Value, kind: &str, name: Option<&str>) -> Option<String> {
    let found = state["resources"].as_array()?.iter().find(|r| r["mode"] == "managed" && r["type"] == kind && name.is_none_or(|n| r["name"] == n) && r["instances"].as_array().is_some_and(|i| !i.is_empty()))?;
    found["name"].as_str().map(String::from)
}

/// The apply that follows creates the dynamic group and policy that let the VM read its state bucket (`vm-state.tf`), and a dynamic group
/// is tenancy-scoped: the Terraform user needs tenancy-level IAM rights, which only an apply can prove. The state proves it for free when
/// it already holds those two resources, or any other dynamic group (`budget-guard.tf` makes one at the tenancy the same way).
fn iam_check(state: &Value) -> Value {
    if state_holds(state, "oci_identity_dynamic_group", Some("vm_state")).is_some() && state_holds(state, "oci_identity_policy", Some("vm_state")).is_some() {
        return check("iam", true, "the VM state dynamic group and policy are in the state: the apply creates no tenancy-level IAM".into(), "");
    }
    match state_holds(state, "oci_identity_dynamic_group", None) {
        Some(name) => check("iam", true, format!("the state holds the tenancy-scoped dynamic group {name}, so this Terraform user may create one for the VM state bucket"), ""),
        None => check(
            "iam",
            false,
            "the next apply creates the tenancy-scoped dynamic group oci_identity_dynamic_group.vm_state (and its policy), and nothing in the state proves the Terraform user may: without it the recreated VM cannot read its backup and starts no stateful app".into(),
            "give the Terraform user's group `Allow group <group> to manage dynamic-groups in tenancy` and `manage policies in tenancy` (OCI console, Identity), or apply the IAM resources once from an administrator (wf vm terraform exec infra/oracle-cloud apply -target=oci_identity_dynamic_group.vm_state -target=oci_identity_policy.vm_state), then rerun the preflight (docs/runbooks/vm-recreate.md, \"State backup\")",
        ),
    }
}

/// Every app `vm-apps.json` provisions (their own groups) and the Terraform group, each checked for presence and usability.
fn secrets_check(repo: &Path) -> Value {
    let listed = fs::read_to_string(repo.join("infra/oracle-cloud/vm-apps.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
    let Some(listed) = listed else {
        return check("secrets", false, "infra/oracle-cloud/vm-apps.json is missing or not JSON".into(), "run from the repository root");
    };
    let mut apps: Vec<String> = listed.as_array().into_iter().flatten().filter_map(|a| a["name"].as_str().map(String::from)).collect();
    apps.push("oracle-terraform".into());
    // the restic password the state backup is made and read with
    apps.push("vm-state-backup".into());
    let mut failed = Vec::new();
    for app in &apps {
        let extra: &[&str] = match app.as_str() {
            "oracle-terraform" => &TERRAFORM_EXTRAS,
            // not in the group (the container never holds it), but `wf ship ci-runner` mints the runner's registration token with it
            "ci-runner" => &["VM_PROVISION_GITHUB_TOKEN"],
            _ => &[],
        };
        if let Err(why) = secrets::verify_for(app, extra) {
            failed.push(why.lines().take(4).collect::<Vec<_>>().join("; "));
        }
    }
    if failed.is_empty() {
        return check("secrets", true, format!("{} secret groups present and usable ({})", apps.len(), apps.join(", ")), "");
    }
    let mint = if failed.iter().any(|f| f.contains("VM_STATE_RESTIC_PASSWORD")) { "; VM_STATE_RESTIC_PASSWORD is minted once (never replaced): openssl rand -base64 32 | tr '+/' '-_' | tr -d '=\\n' | wf secret create VM_STATE_RESTIC_PASSWORD <PROJECT_ID> --value-stdin --apply (docs/runbooks/vm-recreate.md, \"State backup\")" } else { "" };
    check("secrets", false, failed.join(" | "), &format!("wf secret verify --for <app>, then store the missing value in Bitwarden (never in a file){mint}"))
}

/// Every app with state volumes has a backup under a day old that downloads, decrypts and matches its manifest (a verify on agent-shell).
fn backups_check(repo: &Path) -> Value {
    match crate::vm_state::backups_current(repo) {
        Err(why) => check("backups", false, why, "run from the repository root"),
        Ok(problems) if problems.is_empty() => check("backups", true, "every app with state volumes has a verified backup under 24 hours old (or none declares any)".into(), ""),
        Ok(problems) => check("backups", false, problems.join(" | "), "wf vm state backup, then wf vm state verify; see docs/runbooks/vm-recreate.md (\"State backup\")"),
    }
}

/// The tailnet admin API answers with credentials that can list and delete node records (the write scope is probed), so a failure cannot first appear after the VM is destroyed.
fn tailnet_check() -> Value {
    match crate::tailscale::check_api_access() {
        Ok(n) => check("tailnet", true, format!("the tailnet API lists {n} device(s) and the client may delete them (write scope proven)"), ""),
        Err(why) => check("tailnet", false, format!("the tailnet API is not usable: {why}"), "fix TS_VM_OAUTH_CLIENT_SECRET / TS_TAILNET_ID in the vault (docs/runbooks/vm-recreate.md), then rerun the preflight"),
    }
}

/// No workflow run is queued or running: a recreate takes the runner down with it.
fn ci_check() -> Value {
    let mut runs = Vec::new();
    for status in IN_FLIGHT {
        let path = format!("repos/{{owner}}/{{repo}}/actions/runs?status={status}&per_page=100");
        let out = match Command::new("gh").args(["api", &path]).stdin(Stdio::null()).output() {
            Ok(o) => o,
            Err(e) => return check("ci", false, format!("gh could not run: {e}"), "install gh or set GH_TOKEN"),
        };
        if !out.status.success() {
            return check("ci", false, format!("gh api {path} failed: {}", String::from_utf8_lossy(&out.stderr).trim()), "run the preflight where the Actions API is reachable (the VM's runner or a laptop)");
        }
        let body: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
        for r in body["workflow_runs"].as_array().into_iter().flatten() {
            runs.push(format!("{} #{} ({status}, {})", r["name"].as_str().unwrap_or("run"), r["id"], r["head_sha"].as_str().unwrap_or("").chars().take(9).collect::<String>()));
        }
    }
    if runs.is_empty() {
        return check("ci", true, "no workflow run is queued or in progress".into(), "");
    }
    check("ci", false, format!("{} run(s) in flight: {}", runs.len(), runs.join("; ")), "wait for them, or `gh run cancel <id>`, then rerun the preflight")
}

/// The plan of the recreate (`-replace` on the VM, as `apply --recreate` does): its actions per resource, what it destroys, and that this is only what is expected.
fn plan_check(dir: &str, vars: &[(String, String)], expect: &[String]) -> Res<(Value, Value)> {
    let compartment = plain(secret("OCI_COMPARTMENT_OCID"))?.filter(|v| !v.is_empty()).ok_or_else(|| Fail::Plain("OCI_COMPARTMENT_OCID is in neither the environment nor the vault".into()))?;
    let changes = plan(dir, &[&format!("-replace={VM}")], vars)?;
    let acted: Vec<&Change> = changes.iter().filter(|c| !c.actions.iter().all(|a| a == "no-op")).collect();
    let destroys: Vec<&str> = acted.iter().filter(|c| c.actions.iter().any(|a| a == "delete")).map(|c| c.address.as_str()).collect();
    let allowed = |a: &str| EXPECTED_DESTROYS.contains(&a) || expect.iter().any(|e| e == a);
    let unexpected: Vec<&str> = destroys.iter().copied().filter(|a| !allowed(a)).collect();
    let after_apply: Vec<Value> = MINTED
        .iter()
        .filter(|(resource, ..)| destroys.contains(resource))
        .map(|(resource, output, key)| json!({ "after": resource, "run": format!("wf vm terraform exec infra/oracle-cloud output -raw {output} | wf secret update {key} --value-stdin --apply"), "then": "wf ship libs-proxy --ref dev" }))
        .collect();
    let plan = json!({
        "changes": acted.iter().map(|c| json!({ "address": c.address, "actions": c.actions })).collect::<Vec<_>>(),
        "destroys": destroys,
        "unexpectedDestroys": unexpected,
        "afterApply": after_apply,
    });
    let verdict = if !destroys.contains(&VM) {
        check("plan", false, format!("the plan with -replace does not replace {VM}; the state or the configuration is not what the recreate assumes"), "wf vm terraform exec infra/oracle-cloud plan")
    } else if !unexpected.is_empty() {
        check("plan", false, format!("the recreate would also destroy: {}", unexpected.join(", ")), "find what forces each replacement with `wf vm terraform exec infra/oracle-cloud plan`; fix the drift, or pass --expect <address> when it is intended")
    } else if let Some(reason) = guard(&changes, &compartment)? {
        check("plan", false, reason, "stay inside the Always-Free envelope (infra/oracle-cloud/always-free-quota.tf)")
    } else {
        check("plan", true, format!("{} change(s), destroying {}", acted.len(), destroys.join(", ")), "")
    };
    Ok((verdict, plan))
}

/// The preflight's verdict as JSON on stdout; exit 0 only when every check passed.
pub(super) fn run(dir: &str, expect: &[String]) -> Res<i32> {
    let mut checks = Vec::new();
    let mut plan = Value::Null;
    match vm_vars() {
        Err(f) => checks.push(check("inputs", false, message(f), "store the missing secret in Bitwarden")),
        Ok(vars) => {
            let mut pulled = Value::Null;
            checks.push(state(dir, &vars, &mut pulled).unwrap_or_else(|f| check("tfstate", false, message(f), "see the message")));
            if !pulled.is_null() {
                checks.push(iam_check(&pulled));
            }
            checks.push(secrets_check(&repo_root()));
            checks.push(backups_check(&repo_root()));
            checks.push(tailnet_check());
            checks.push(ci_check());
            match plan_check(dir, &vars, expect) {
                Ok((verdict, p)) => {
                    checks.push(verdict);
                    plan = p;
                }
                Err(f) => checks.push(check("plan", false, message(f), "wf vm terraform exec infra/oracle-cloud plan")),
            }
        }
    }
    let ok = checks.iter().all(|c| c["ok"] == true);
    let next = if ok { "wf vm terraform apply --recreate" } else { "fix the failed checks, then rerun wf vm terraform preflight" };
    println!("{}", json!({ "ok": ok, "checks": checks, "plan": plan, "next": next }));
    Ok(i32::from(!ok))
}
