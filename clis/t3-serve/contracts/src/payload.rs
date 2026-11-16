//! The request payloads of the RPCs below, decoded before any handler runs the way the pinned server's Effect schema
//! as the pinned server's Effect schema refuses them: the first field in declaration order that is missing or of the wrong type is a defect, never a typed error.
use serde_json::Value;

/// Fields in the order the contract's struct declares them: `name:kind`, kind `s` string, `n` number, `b` boolean, `a` array, `o` object, `sn` string or null, `p(day|http|npm)` a string matching a contract pattern, `l(x|y)` one of
/// the literals, a trailing `?` for an optional key. What a field holds beyond its kind (a string's trim and length, an array's items) is the handler's.
const SCHEMAS: &[(&str, &str)] = &[
    ("provider.install.start", "instanceId:s"),
    ("provider.install.cancel", "instanceId:s operationId:s"),
    ("provider.install.subscribe", "instanceId:s"),
    ("provider.install.remove", "instanceId:s"),
    ("server.searchAcpRegistry", "query:s limit:n?"),
    ("server.prepareAcpRegistryAgent", "agentId:s"),
    ("server.uninstallAcpRegistryManagedBinary", "agentId:s"),
    ("server.acceptAcpRegistryUrlAuth", "instanceId:s elicitationId:s"),
    ("server.listAcpRegistrySessions", "instanceId:s projectId:s cursor:s?"),
    ("server.importAcpRegistrySession", "instanceId:s projectId:s sessionId:s"),
    ("server.deleteAcpRegistrySession", "instanceId:s projectId:s sessionId:s"),
    ("server.listAcpRegistryProviders", "instanceId:s projectId:s"),
    ("server.setAcpRegistryProvider", "instanceId:s projectId:s providerId:s apiType:s baseUrl:s"),
    ("server.disableAcpRegistryProvider", "instanceId:s projectId:s providerId:s"),
    ("server.logoutAcpRegistry", "instanceId:s"),
    ("provider.auth.start", "instanceId:s methodId:s? returnUrl:s? callbackMode:l(server|client)?"),
    ("provider.auth.respond", "instanceId:s flowId:s interactionId:s"),
    ("provider.auth.complete", "instanceId:s flowId:s callbackUrl:s"),
    ("provider.auth.cancel", "instanceId:s flowId:s"),
    ("provider.auth.logout", "instanceId:s"),
    ("provider.auth.subscribe", "instanceId:s"),
    ("provider.chatgpt.reconnect-profile", "instanceId:s methodId:s"),
    ("provider.chatgpt.import-profile", "instanceId:s profile:o"),
    ("provider.chatgpt.handoff.subscribe", "instanceId:s environmentId:s attemptId:s returnUrl:s"),
    ("server.updateProvider", "provider:s targetVersion:s? instanceId:s?"),
    ("projects.searchEntries", "cwd:s query:s limit:n kind:l(file|directory)? imageOnly:b?"),
    ("projects.searchContents", "cwd:s query:s limit:n caseSensitive:b wholeWord:b useRegex:b"),
    ("projects.listIgnoredFiles", "cwd:s"),
    ("projects.writeFile", "cwd:s relativePath:s contents:s"),
    ("projects.createNew", "name:s"),
    ("pullRequests.routingIdentity", "host:s"),
    ("pullRequests.linkedThreads", "projectId:s host:s? expectedAccountId:s? allowStale:b? repository:s number:n"),
    ("pullRequests.generateReviewGuide", "projectId:s host:s? expectedAccountId:s? allowStale:b? repository:s number:n"),
    ("pullRequests.comment", "projectId:s host:s? expectedAccountId:s? allowStale:b? repository:s number:n body:s"),
    ("pullRequests.update", "projectId:s host:s? expectedAccountId:s? allowStale:b? repository:s number:n title:s? body:s? maintainerCanModify:b?"),
    ("pullRequests.updateComment", "projectId:s host:s? expectedAccountId:s? allowStale:b? repository:s number:n commentId:s kind:l(issue-comment|review-comment) body:s"),
    ("pullRequests.submitReview", "projectId:s host:s? expectedAccountId:s? allowStale:b? repository:s number:n verdict:l(comment|approve|request-changes) body:s comments:a"),
    ("pullRequests.replyToThread", "projectId:s host:s? expectedAccountId:s? allowStale:b? repository:s number:n threadId:s body:s"),
    ("pullRequests.setThreadResolution", "projectId:s host:s? expectedAccountId:s? allowStale:b? repository:s number:n threadId:s resolved:b"),
    ("pullRequests.setReaction", "projectId:s host:s? expectedAccountId:s? allowStale:b? repository:s number:n subjectId:s? content:l(thumbs-up|thumbs-down|laugh|hooray|confused|heart|rocket|eyes) reacted:b"),
    ("pullRequests.runAction", "stackNumber:n? expectedStackHeads:a? projectId:s host:s? expectedAccountId:s? allowStale:b? repository:s number:n action:l(merge|ready|draft|close|reopen|delete-branch|update-branch|enable-auto-merge|disable-auto-merge|revert|approve-workflows) mergeMethod:l(merge|squash|rebase)? updateMethod:l(merge|rebase)? replyWithSummary:b?"),
    ("server.getProcessResourceHistory", "windowMs:n bucketMs:n"),
    ("server.getResourceTelemetryHistory", "windowMs:n bucketMs:n"),
    ("server.signalProcess", "pid:n startTimeMs:n signal:l(SIGINT|SIGKILL)"),
    ("server.checkOtlpEndpoint", "signal:l(traces|metrics|logs) url:p(http)"),
    ("server.getNpmRelease", "spec:p(npm)"),
    ("server.getUsageSummary", "sinceDay:p(day) untilDay:p(day) timeZone:s resolution:l(day|hour)? sinceTime:s? untilTime:s? awaitRefresh:b?"),
    ("shell.openInEditor", "cwd:s editor:l(cursor|trae|kiro|vscode|vscode-insiders|vscodium|zed|antigravity|idea|aqua|clion|datagrip|dataspell|goland|phpstorm|pycharm|rider|rubymine|rustrover|webstorm|file-manager) reveal:b?"),
    ("peerLinks.unlink", "environmentId:s"),
    ("preview.clearProfile", "profileId:s"),
    ("preview.stopLocalServer", "pid:n port:n"),
    ("scheduledTasks.listWebhookDeliveries", "id:s acceptAnyOutcome:b?"),
    ("scheduledTasks.getWebhookDelivery", "id:s deliveryId:s acceptAnyOutcome:b?"),
    ("attachments.delete", "attachmentId:s"),
    ("vcs.init", "cwd:s kind:l(git|jj|unknown)?"),
    ("vcs.listRefs", "cwd:s query:s? cursor:n? includeMatchingRemoteRefs:b? refKind:l(all|local|remote)? refresh:b? limit:n?"),
    ("vcs.createRef", "cwd:s refName:s switchRef:b?"),
    ("vcs.switchRef", "cwd:s refName:s"),
    ("vcs.pull", "cwd:s"),
    ("vcs.refreshBranch", "cwd:s"),
    ("subscribeVcsBranch", "cwd:s"),
    ("vcs.removeWorktree", "cwd:s path:s force:b?"),
    ("git.resolvePullRequest", "cwd:s reference:s"),
    ("git.preparePullRequestThread", "cwd:s reference:s mode:l(local|worktree) threadId:s?"),
    ("git.runStackedAction", "actionId:s cwd:s action:l(commit|push|create_pr|commit_push|commit_push_pr) commitMessage:s? featureBranch:b? filePaths:a? threadId:s? projectId:s?"),
    ("review.getDiffFileContents", "cwd:s sourceKind:l(working-tree|branch-range|commit) changeType:l(change|rename-pure|rename-changed|new|deleted) baseRef:sn headRef:sn oldPath:s newPath:s"),
    ("sourceControl.lookupRepository", "provider:s repository:s cwd:s?"),
    ("sourceControl.cloneRepository", "provider:s? repository:s? remoteUrl:s? destinationPath:s protocol:l(auto|ssh|https)?"),
    ("sourceControl.signInStart", "provider:s host:s"),
    ("sourceControl.signInCancel", "provider:s host:s"),
    ("sourceControl.signInWait", "provider:s host:s"),
    ("sourceControl.appAccess", "provider:s host:s repository:s"),
    ("sourceControl.starred", "provider:s host:s repository:s"),
    ("sourceControl.publishRepository", "cwd:s provider:s repository:s visibility:l(private|public) remoteName:s? protocol:l(auto|ssh|https)?"),
    ("review.getBranchCommits", "cwd:s baseRef:s?"),
    ("review.getDiffPreview", "cwd:s baseRef:s? commitSha:s? ignoreWhitespace:b? sourceKind:l(working-tree|branch-range)? trackedOnly:b? metadataOnly:b? file:o?"),
];

/// The patterns the contracts check strings against: the text the decoder names, and the match.
fn pattern(name: &str) -> (&'static str, fn(&str) -> bool) {
    match name {
        "day" => (r"^\d{4}-\d{2}-\d{2}$", |v| v.len() == 10 && v.bytes().enumerate().all(|(i, b)| if i == 4 || i == 7 { b == b'-' } else { b.is_ascii_digit() })),
        "http" => (r"^https?:\/\/.", |v| {
            let v = v.to_ascii_lowercase();
            v.strip_prefix("https://").or_else(|| v.strip_prefix("http://")).is_some_and(|rest| !rest.is_empty())
        }),
        _ => (r"^(?:latest|nightly|\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)$", |v| {
            let (core, suffix) = v.split_once('-').map_or((v, None), |(c, s)| (c, Some(s)));
            let numeric = core.split('.').count() == 3 && core.split('.').all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
            v == "latest" || v == "nightly" || (numeric && suffix.is_none_or(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')))
        }),
    }
}

fn expected(kind: &str) -> String {
    match kind {
        "n" => "number".into(),
        "b" => "boolean".into(),
        "a" => "array".into(),
        "o" => "object".into(),
        "sn" => "string | null".into(),
        k if k.starts_with("p(") => format!("a string matching the RegExp {}", pattern(&k[2..k.len() - 1]).0),
        k if k.starts_with("l(") => k[2..k.len() - 1].split('|').map(|v| format!("\"{v}\"")).collect::<Vec<_>>().join(" | "),
        _ => "string".into(),
    }
}

fn is(kind: &str, value: &Value) -> bool {
    match kind {
        "n" => value.is_number(),
        "b" => value.is_boolean(),
        "a" => value.is_array(),
        "o" => value.is_object(),
        "sn" => value.is_string() || value.is_null(),
        k if k.starts_with("p(") => value.is_string(),
        k if k.starts_with("l(") => value.as_str().is_some_and(|v| k[2..k.len() - 1].split('|').any(|l| l == v)),
        _ => value.is_string(),
    }
}

/// The smallest payload that decodes as the schema of `tag`: every required field with a value of its kind.
pub fn minimal(tag: &str) -> Option<Value> {
    let (_, schema) = SCHEMAS.iter().find(|(t, _)| *t == tag)?;
    let fields = schema.split(' ').filter_map(|f| f.split_once(':')).filter(|(_, kind)| !kind.ends_with('?')).map(|(name, kind)| {
        let value = match kind {
            "n" => Value::from(1),
            "b" => Value::Bool(true),
            "a" => Value::Array(Vec::new()),
            "o" => Value::Object(Default::default()),
            "sn" => Value::Null,
            "p(day)" => Value::from("2026-01-01"),
            "p(http)" => Value::from("http://a"),
            "p(npm)" => Value::from("latest"),
            k if k.starts_with("l(") => Value::from(k[2..k.len() - 1].split('|').next().unwrap_or("")),
            _ => Value::from("a"),
        };
        (name.to_string(), value)
    });
    Some(Value::Object(fields.collect()))
}

/// The defect text when `payload` does not decode as the schema of `tag`; `None` when it does or the tag has no schema here.
pub fn defect(tag: &str, payload: &Value) -> Option<String> {
    let (_, schema) = SCHEMAS.iter().find(|(t, _)| *t == tag)?;
    let body = payload.as_object()?;
    for field in schema.split(' ') {
        let (name, kind) = field.split_once(':')?;
        let (kind, optional) = kind.strip_suffix('?').map_or((kind, false), |k| (k, true));
        match body.get(name) {
            None if optional => {}
            None => return Some(format!("Missing key\n  at [\"{name}\"]")),
            Some(v) if !is(kind, v) => return Some(format!("Expected {}\n  at [\"{name}\"]", if kind.starts_with("p(") { "string".into() } else { expected(kind) })),
            Some(v) if kind.starts_with("p(") && !v.as_str().is_some_and(pattern(&kind[2..kind.len() - 1]).1) => return Some(format!("Expected {}\n  at [\"{name}\"]", expected(kind))),
            Some(_) => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_first_field_that_does_not_decode_is_the_defect() {
        // Given a schema of two string fields
        let tag = "provider.install.cancel";
        // When the payload is empty, then the first key is missing; when it has the first and not the second, the second is
        assert_eq!(defect(tag, &json!({})).unwrap(), "Missing key\n  at [\"instanceId\"]");
        assert_eq!(defect(tag, &json!({"instanceId": "a"})).unwrap(), "Missing key\n  at [\"operationId\"]");
        // And a number where a string belongs names that field, and a whole payload decodes
        assert_eq!(defect(tag, &json!({"instanceId": 7, "operationId": "a"})).unwrap(), "Expected string\n  at [\"instanceId\"]");
        assert_eq!(defect(tag, &json!({"instanceId": "a", "operationId": "a"})), None);
    }

    #[test]
    fn the_minimal_payload_of_every_schema_decodes() {
        for (tag, _) in SCHEMAS {
            let minimal = minimal(tag).unwrap();
            assert_eq!(defect(tag, &minimal), None, "{tag}: {minimal}");
        }
    }

    #[test]
    fn a_string_that_misses_a_contract_pattern_is_the_decoders_pattern_defect() {
        let usage = |day: &str| defect("server.getUsageSummary", &json!({"sinceDay": day, "untilDay": "2026-01-01", "timeZone": "UTC"}));
        assert_eq!(usage("a").unwrap(), "Expected a string matching the RegExp ^\\d{4}-\\d{2}-\\d{2}$\n  at [\"sinceDay\"]");
        assert_eq!(usage("2026-02-30"), None);
        let release = |spec: &str| defect("server.getNpmRelease", &json!({"spec": spec}));
        assert_eq!((release("latest"), release("1.2.3"), release("1.2.3-rc.1")), (None, None, None));
        assert!(release("1.2").is_some() && release("a").is_some());
        assert_eq!(defect("server.checkOtlpEndpoint", &json!({"signal": "logs", "url": "a"})).unwrap(), "Expected a string matching the RegExp ^https?:\\/\\/.\n  at [\"url\"]");
    }

    #[test]
    fn a_tag_without_a_schema_here_is_left_to_its_handler() {
        assert_eq!(defect("server.probe", &json!({})), None);
    }
}
