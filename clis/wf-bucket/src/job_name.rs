//! A job's name as the Actions API renders it: `<caller job> / <job>` when a reusable workflow ran it, and a matrix job's parameters in
//! parentheses (`deploy (libs-proxy)`, `flow-tests (2/4)`). Every comparison against a job name goes through here.

/// Where the parameters group a matrix job's name ends with starts (at the space before it).
fn parameters_at(raw: &str) -> Option<usize> {
    if raw.ends_with(')') { raw.rfind(" (") } else { None }
}

/// The job's own name: everything up to the last ` / ` of the name proper dropped, the matrix parameters kept.
pub fn job_name(raw: &str) -> &str {
    let core = &raw[..parameters_at(raw).unwrap_or(raw.len())];
    core.rfind(" / ").map_or(raw, |at| &raw[at + 3..])
}

/// The job without its shard suffix: `flow-tests` for `run / flow-tests (2/4)`. Other matrix parameters stay.
pub fn stage(raw: &str) -> &str {
    let name = job_name(raw);
    let shard = parameters_at(name).filter(|at| {
        let inner = &name[at + 2..name.len() - 1];
        inner.split_once('/').is_some_and(|(i, n)| [i, n].iter().all(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())))
    });
    shard.map_or(name, |at| &name[..at])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_called_workflows_prefix_is_dropped_and_a_plain_name_is_kept() {
        assert_eq!((job_name("run / flow-tests"), job_name("flow-tests"), job_name("a / b / c")), ("flow-tests", "flow-tests", "c"));
    }

    #[test]
    fn matrix_parameters_survive_and_a_slash_inside_them_is_not_a_prefix() {
        assert_eq!((job_name("run / deploy (libs-proxy)"), job_name("deploy (a / b)"), job_name("run / deploy (a / b)")), ("deploy (libs-proxy)", "deploy (a / b)", "deploy (a / b)"));
    }

    #[test]
    fn a_shard_suffix_is_dropped_by_stage_only() {
        assert_eq!((stage("run / flow-tests (2/4)"), stage("flow-tests (1/1)"), stage("run / deploy (libs-proxy)"), stage("flow-tests")), ("flow-tests", "flow-tests", "deploy (libs-proxy)", "flow-tests"));
        assert_eq!(job_name("run / flow-tests (2/4)"), "flow-tests (2/4)");
    }
}
