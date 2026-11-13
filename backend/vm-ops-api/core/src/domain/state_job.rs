//! The two state jobs CI may start on agent-shell: the nightly restic backup of the VM's state volumes and the verification of its newest
//! snapshots. The script is fixed here, so a caller names only the verb; the plan (which volumes, how big) is the `vm-apps.json` of `dev`,
//! which the script fetches itself, so nothing the caller sends reaches agent-shell.

pub const VERBS: [&str; 2] = ["backup", "verify"];

const APPS_URL: &str = "https://raw.githubusercontent.com/nuesslerm/mobile-learning-app/dev/infra/oracle-cloud/vm-apps.json";
const SCRATCH: &str = "/scratch";

pub fn is_verb(text: &str) -> bool {
    VERBS.contains(&text)
}

/// The script agent-shell runs for `verb` (one of `VERBS`): the same guard and scratch as `wf vm state` (`clis/deploy/src/vm_state.rs`), with the
/// plan read from the downloaded `vm-apps.json` (`--apps-file`) instead of passed in.
pub fn script(verb: &str) -> String {
    format!(
        "W=/workspace/home/wf-bin/wf; [ -x $W ] || W=wf\n\
S=${{WF_VM_STATE_SCRATCH:-{SCRATCH}}}\n\
if [ -z \"$WF_VM_STATE_SCRATCH\" ] && ! grep -q ' {SCRATCH} tmpfs ' /proc/mounts; then echo '{{\"event\":\"vm-state\",\"verb\":\"{verb}\",\"ok\":false,\"apps\":[],\"error\":\"agent-shell has no tmpfs at {SCRATCH}\",\"next\":\"redeploy agent-shell\"}}'; exit 1; fi\n\
D=$S/wf-vm-state-run.$$\n\
trap 'rm -rf \"$D\"' EXIT HUP INT TERM\n\
umask 077; mkdir -p \"$D\"; echo $$ > \"$D/.wf-pid\"\n\
curl -fsSL --max-time 60 '{APPS_URL}' -o \"$D/vm-apps.json\" || {{ echo '{{\"event\":\"vm-state\",\"verb\":\"{verb}\",\"ok\":false,\"apps\":[],\"error\":\"vm-apps.json of dev could not be fetched\",\"next\":\"retry\"}}'; exit 1; }}\n\
TMPDIR=\"$D\" SQLITE_TMPDIR=\"$D\" WF_VM_STATE_SWEEP_ROOT=\"$S\" WF_SITE=vm $W vm state {verb} --local --apps-file \"$D/vm-apps.json\"\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_the_verbs_when_checked_then_only_backup_and_verify_are_state_jobs() {
        assert!(is_verb("backup") && is_verb("verify"));
        for bad in ["", "restore", "list", "backup;id", "Backup", "backup "] {
            assert!(!is_verb(bad), "{bad:?}");
        }
    }

    #[test]
    fn given_a_verb_when_the_script_is_rendered_then_it_runs_that_verb_on_the_fetched_plan_and_nothing_a_caller_chose() {
        let script = script("backup");

        assert!(script.contains("vm state backup --local --apps-file \"$D/vm-apps.json\""), "{script}");
        assert!(script.contains(APPS_URL), "{script}");
        assert!(script.contains("grep -q ' /scratch tmpfs ' /proc/mounts"), "the tmpfs guard: {script}");
        assert!(!script.contains("--plan"), "{script}");
    }
}
