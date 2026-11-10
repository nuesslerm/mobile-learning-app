//! `wf agent-config sync ...`: the agent-config-sync CLI (`sync [--check]`, `mcp-guard`), run in this process
//! through the library crate instead of spawning node twice. Failures print the child's stderr and
//! then wf's own `wf: <tail>` line, as the TypeScript passthrough does.
const STDERR_TAIL_BYTES: usize = 4096;

/// What `wf` mounts: `agent-config sync [--check]` and the hook plumbing `agent-config mcp-guard`.
pub const COMMANDS: &[(&str, wf_kit::cli::Handler)] = &[("agent-config", run)];

pub fn run(args: &[String]) -> Option<i32> {
    let outcome = crate::cli::run(args, &mut std::io::stdin());
    print!("{}", outcome.stdout);
    eprint!("{}", outcome.stderr);
    if outcome.code != 0 {
        let chars: Vec<char> = outcome.stderr.chars().collect();
        let tail: String = chars[chars.len().saturating_sub(STDERR_TAIL_BYTES)..]
            .iter()
            .collect();
        let message = if tail.is_empty() {
            format!("passthrough command failed with exit code {}", outcome.code)
        } else {
            tail
        };
        wf_kit::wf_err!("wf: {message}");
    }
    Some(outcome.code)
}
