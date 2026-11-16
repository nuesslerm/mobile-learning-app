//! The request payloads of the RPCs below, decoded before any handler runs the way the pinned server's Effect schema
//! as the pinned server's Effect schema refuses them: the first field in declaration order that is missing or of the wrong type is a defect, never a typed error.
use serde_json::{Map, Value};

/// Fields in the order the contract's struct declares them: `name:kind`, kind `s` string, `n` number, `b` boolean, `a` array, `o` object, `sn` string or null, `p(day|http|npm)` a string matching a contract pattern, `l(x|y)` one of
/// the literals, a trailing `?` for an optional key, a trailing `~` for an optional key that also takes null (its defect names `| null`). What a field holds beyond its kind (a string's trim and length, an array's items) is the handler's.
const SCHEMAS: &[(&str, &str)] = &[
    ("provider.install.start", "instanceId:s"),
    ("provider.install.cancel", "instanceId:s operationId:s"),
    ("provider.install.subscribe", "instanceId:s"),
    ("provider.install.remove", "instanceId:s"),
    ("server.searchAcpRegistry", "query:s limit:r(1,512)?"),
    ("server.removeKeybinding", "key:s command:kc when:s?"),
    ("server.upsertKeybinding", "key:s command:kc when:s? replace:o(keybindingRule)?"),
    ("server.reportClientActivity", "environmentId:s? clientId:s clientKind:l(web|desktop-renderer|mobile|unknown) visible:b focused:b recentlyInteracted:b appState:l(active|inactive|background|unknown)? lowPowerMode:l(true|false|unknown)? scopes:a ttlMs:n? observedAt:dt"),
    ("server.prepareTriage", "client:o"),
    ("server.listTextGeneratorModels", "id:s config:o(textGenerator)? refresh:b? test:b? model:s?"),
    ("server.updateSettings", "patch:o(settingsPatch) providerInstanceMutation:o?"),
    ("orchestration.getWorktreeThreadIds", "worktreePath:s"),
    ("orchestration.getThreadProjection", "threadId:s"),
    ("orchestration.getThreadTitles", "threadIds:a"),
    ("orchestration.getThreadLinkMetadata", "threadIds:a"),
    ("orchestration.getOlderSettledThreads", "before:o(settledCursor) projectIds:a? limit:r(0,50)"),
    ("orchestration.getTurnDiff", "fromTurnCount:n toTurnCount:n threadId:s ignoreWhitespace:b?"),
    ("orchestration.searchThread", "threadId:s skills:a? query:s"),
    ("orchestration.searchThreadStream", "threadId:s skills:a? query:s"),
    ("orchestration.subscribeShell", "afterSequence:n?"),
    ("orchestration.subscribeThread", "threadId:s afterSequence:n? requestCompletionMarker:b? acceptBoundedSnapshot:b? acceptDeferredWork:b? acceptCompactTurnItems:b? acceptDedupedBodies:b? acceptCompactIds:b?"),
    ("orchestration.launchThread", "commandId:s creationSource:l(web|mobile|mcp|provider|server)? threadId:s? reuseExistingThread:b? projectId:s title:s generateTitle:b? modelSelection:ms runtimeMode:l(approval-required|auto-accept-edits|auto|full-access) interactionMode:l(default|plan) workspaceStrategy:x initialMessage:x?"),
    ("device.configure", "enabled:b~ agentAccessEnabled:b~ onboardingCompleted:b~"),
    ("device.list", "updateTool:l(hub|agent)~ inspectOnly:b~ retryHostId:s~"),
    ("device.testHost", "kind:l(ssh)~ id:s label:s target:s identityFile:s~ port:r(1,65535)~"),
    ("device.easAccount", "hostId:s~ accessToken:x~"),
    ("device.simlockIdentity", "gatewayUrl:s~ hostId:s~ agentToken:x~"),
    ("device.open", "threadId:s hostId:s~ deviceId:s platform:l(ios|android) boot:b~"),
    ("device.close", "hostId:s~ threadId:s deviceId:s~ shutdown:b~"),
    ("device.shutdown", "hostId:s~ deviceId:s platform:l(ios|android)"),
    ("device.detail", "hostId:s~ deviceId:s"),
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

/// Structs a field of kind `o(name)` or `a(name)` holds, declared like `SCHEMAS`; only the fields the pinned contracts declare first are decoded for the settings patch.
const NESTED: &[(&str, &str)] = &[
    ("keybindingRule", "key:s command:kc when:s?"),
    ("textGenerator", "kind:l(openaiCompatible|anthropic) label:s? baseUrl:p(url) enabled:b?"),
    ("settingsPatch", "setModelFavorites:a(favoriteItem)? setModelsHidden:a(hiddenItem)? setProviderModelOrder:o(modelOrder)?"),
    ("settledCursor", "settledAt:s threadId:s?"),
    ("favoriteItem", "provider:s model:s favorite:b"),
    ("hiddenItem", "provider:s model:s hidden:b"),
    ("modelOrder", "provider:s modelOrder:a"),
];

/// The commands of `orchestration.dispatchCommand` whose union member is decoded here, fields in the order the member declares them (the creation fields and
/// `ThreadPullRequestKey` spread in place). Beyond the kinds above: `dt` a UTC date-time string, `ms` a model selection, `x` any value that is present.
const COMMANDS: &[(&str, &str)] = &[
    ("thread.create", "createdBy:l(user|agent|system) creationSource:l(web|mobile|mcp|provider|server) commandId:s threadId:s projectId:s title:s modelSelection:ms runtimeMode:l(approval-required|auto-accept-edits|auto|full-access) interactionMode:l(default|plan) branch:sn worktreePath:sn linkOrigin:o? delegatedFrom:o? importedNativeThread:o?"),
    ("thread.settle", "commandId:s threadId:s expectedLatestRunId:s? settledAt:dt?"),
    ("thread.auto-settle", "commandId:s threadId:s snapshotAt:dt settledAt:dt?"),
    ("thread.tag.set", "commandId:s threadId:s tag:s tagged:b"),
    ("thread.pull-request.link", "commandId:s threadId:s host:s repository:s number:n url:s source:l(manual|created|agent|stack|stack-dismissed) repositoryWebUrl:s?"),
    ("thread.pull-request.unlink", "commandId:s threadId:s host:s repository:s number:n"),
    ("thread.pull-request-link.sync", "commandId:s threadId:s host:s repository:s number:n snapshot:o stack:x"),
    ("thread.pull-request.watch", "commandId:s threadId:s host:s repository:s number:n watching:b link:o?"),
    ("thread.model-selection.set", "commandId:s threadId:s modelSelection:ms"),
    ("thread.fork", "createdBy:l(user|agent|system) creationSource:l(web|mobile|mcp|provider|server) commandId:s sourceThreadId:s targetThreadId:s sourcePoint:x title:s? sideChat:b? createdAt:dt?"),
    ("thread.merge_back", "createdBy:l(user|agent|system) creationSource:l(web|mobile|mcp|provider|server) commandId:s sourceThreadId:s targetThreadId:s sourcePoint:x createdAt:dt?"),
];

const KEYBINDING_COMMANDS: &str = include_str!("../data/keybinding-commands.txt");

/// A keybinding command is one of the static names or `script.<id>.run`; the union fails with both members' messages.
fn keybinding_command(v: &Value) -> bool {
    v.as_str().is_some_and(|c| {
        KEYBINDING_COMMANDS.lines().any(|n| n == c)
            || c.strip_prefix("script.").and_then(|r| r.strip_suffix(".run")).is_some_and(|id| {
                id.bytes().next().is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit()) && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            })
    })
}

fn keybinding_command_defect(path: &str) -> String {
    let names = KEYBINDING_COMMANDS.lines().map(|n| format!("\"{n}\"")).collect::<Vec<_>>().join(" | ");
    format!("Expected {names}\n  at {path}\nExpected a string matching template literal parts\n  at {path}")
}

/// The bounds of a kind `r(low,high)`, an integer in that range.
fn bounds(kind: &str) -> (i64, i64) {
    let (low, high) = kind[2..kind.len() - 1].split_once(',').unwrap_or(("0", "0"));
    (low.parse().unwrap_or(0), high.parse().unwrap_or(0))
}

/// The patterns the contracts check strings against: the text the decoder names, and the match.
fn pattern(name: &str) -> (&'static str, fn(&str) -> bool) {
    match name {
        "day" => (r"^\d{4}-\d{2}-\d{2}$", |v| v.len() == 10 && v.bytes().enumerate().all(|(i, b)| if i == 4 || i == 7 { b == b'-' } else { b.is_ascii_digit() })),
        "url" => ("<filter>", |v| {
            let v = v.trim();
            let Some((scheme, rest)) = v.split_once("://") else { return v.is_empty() || v.len() > 2048 };
            let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
            v.len() > 2048 || (matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") && !authority.contains('@') && !authority.is_empty() && !v.contains(['?', '#']))
        }),
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
        k if k.starts_with("o(") => "object".into(),
        k if k.starts_with("a(") => "array".into(),
        k if k.starts_with("r(") => "number".into(),
        "sn" => "string | null".into(),
        k if k.starts_with("r(") => {
            let (low, high) = bounds(k);
            format!("a value between {low} and {high}")
        }
        k if k.starts_with("p(url") => "<filter>".into(),
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
        k if k.starts_with("o(") => value.is_object(),
        k if k.starts_with("a(") => value.is_array(),
        k if k.starts_with("r(") => value.is_number(),
        "kc" => keybinding_command(value),
        "sn" => value.is_string() || value.is_null(),
        k if k.starts_with("p(") => value.is_string(),
        k if k.starts_with("l(") => value.as_str().is_some_and(|v| k[2..k.len() - 1].split('|').any(|l| l == v)),
        _ => value.is_string(),
    }
}

/// The smallest payload that decodes as the schema of `tag`: every required field with a value of its kind.
pub fn minimal(tag: &str) -> Option<Value> {
    if tag == "device.action" {
        return Some(serde_json::json!({"deviceId": "a", "type": "shake"}));
    }
    let (_, schema) = SCHEMAS.iter().find(|(t, _)| *t == tag)?;
    Some(minimal_of(schema))
}

fn minimal_of(schema: &str) -> Value {
    let fields = schema.split(' ').filter_map(|f| f.split_once(':')).filter(|(_, kind)| !kind.ends_with(['?', '~'])).map(|(name, kind)| {
        let value = match kind {
            "n" => Value::from(1),
            "b" => Value::Bool(true),
            "a" => Value::Array(Vec::new()),
            "o" => Value::Object(Default::default()),
            "sn" => Value::Null,
            "dt" => Value::from("2026-01-01T00:00:00Z"),
            "kc" => Value::from("chat.new"),
            "ms" => serde_json::json!({"instanceId": "a", "model": "a"}),
            k if k.starts_with("o(") => NESTED.iter().find(|(n, _)| *n == &k[2..k.len() - 1]).map_or_else(|| Value::Object(Default::default()), |(_, nested)| minimal_of(nested)),
            k if k.starts_with("a(") => Value::Array(Vec::new()),
            k if k.starts_with("r(") => Value::from(bounds(k).0),
            "p(url)" => Value::from("http://a"),
            "p(day)" => Value::from("2026-01-01"),
            "p(http)" => Value::from("http://a"),
            "p(npm)" => Value::from("latest"),
            k if k.starts_with("l(") => Value::from(k[2..k.len() - 1].split('|').next().unwrap_or("")),
            _ => Value::from("a"),
        };
        (name.to_string(), value)
    });
    Value::Object(fields.collect())
}

/// The defect text when `payload` does not decode as the schema of `tag`; `None` when it does or the tag has no schema here.
pub fn defect(tag: &str, payload: &Value) -> Option<String> {
    if tag == "device.action" {
        return device_action_defect(payload);
    }
    let (_, schema) = SCHEMAS.iter().find(|(t, _)| *t == tag)?;
    decode(schema, payload.as_object()?)
}

const DEVICE_ACTIONS: [&str; 14] = ["setAppearance", "setTextSize", "setToggle", "setLiquidGlass", "setColorFilter", "setOrientation", "setLocation", "clearLocation", "setPermission", "openUrl", "launchApp", "terminateApp", "shake", "sendPush"];

/// A device action that names none of the union's members fails the whole union; a named member's own fields are the handler's.
fn device_action_defect(payload: &Value) -> Option<String> {
    let named = payload.get("type").and_then(Value::as_str).is_some_and(|t| DEVICE_ACTIONS.contains(&t));
    let members: Vec<String> = DEVICE_ACTIONS.iter().map(|t| format!("{{ readonly \"type\": \"{t}\", ... }}")).collect();
    (!named).then(|| format!("Expected {}", members.join(" | ")))
}

/// How a command of `kind` decodes: `None` when its member is not decoded here, else `Err` with the defect text of the first field that fails.
pub fn command_decoded(kind: &str, body: &Map<String, Value>) -> Option<Result<(), String>> {
    let (_, schema) = COMMANDS.iter().find(|(t, _)| *t == kind)?;
    Some(decode(schema, body).map_or(Ok(()), Err))
}

fn at<S: AsRef<str>>(path: &[S]) -> String {
    path.iter().map(|p| p.as_ref()).map(|p| if p.bytes().all(|b| b.is_ascii_digit()) { format!("[{p}]") } else { format!("[\"{p}\"]") }).collect()
}

fn decode(schema: &str, body: &Map<String, Value>) -> Option<String> {
    decode_in(schema, body, &[])
}

fn decode_in(schema: &str, body: &Map<String, Value>, prefix: &[String]) -> Option<String> {
    for field in schema.split(' ') {
        let (name, kind) = field.split_once(':')?;
        let (kind, nullable) = kind.strip_suffix('~').map_or((kind, false), |k| (k, true));
        let (kind, optional) = kind.strip_suffix('?').map_or((kind, nullable), |k| (k, true));
        let key: Vec<String> = prefix.iter().cloned().chain([name.to_string()]).collect();
        match body.get(name) {
            None if optional => {}
            None => return Some(format!("Missing key\n  at {}", at(&key))),
            Some(Value::Null) if nullable => {}
            Some(v) if nullable && kind != "x" && !(if kind.starts_with("r(") { v.is_number() } else { is(kind, v) }) => return Some(format!("Expected {} | null\n  at {}", if kind.starts_with("r(") { "number".into() } else { expected(kind) }, at(&key))),
            Some(v) if kind == "ms" => {
                if let Some(d) = model_selection(v, name) {
                    return Some(d);
                }
            }
            Some(v) if kind == "dt" => match v.as_str() {
                None => return Some(format!("Expected string\n  at {}", at(&key))),
                Some(s) if !utc_date_time(s) => return Some(format!("Expected a valid UTC DateTime string\n  at {}", at(&key))),
                Some(_) => {}
            },
            Some(_) if kind == "x" => {}
            Some(v) if kind == "kc" => {
                if !keybinding_command(v) {
                    return Some(keybinding_command_defect(&at(&key)));
                }
            }
            Some(v) if kind.starts_with("r(") => {
                let (low, high) = bounds(kind);
                match v.as_f64() {
                    None => return Some(format!("Expected number\n  at {}", at(&key))),
                    Some(n) if n.fract() == 0.0 && (n < low as f64 || n > high as f64) => return Some(format!("Expected a value between {low} and {high}\n  at {}", at(&key))),
                    Some(_) => {}
                }
            }
            Some(v) if kind.starts_with("o(") || kind.starts_with("a(") => {
                if !is(kind, v) {
                    return Some(format!("Expected {}\n  at {}", expected(kind), at(&key)));
                }
                let nested = NESTED.iter().find(|(n, _)| *n == &kind[2..kind.len() - 1]).map_or("", |(_, s)| *s);
                if kind.starts_with("o(") {
                    if let Some(d) = decode_in(nested, v.as_object()?, &key) {
                        return Some(d);
                    }
                } else {
                    for (i, item) in v.as_array()?.iter().enumerate() {
                        let at_item: Vec<String> = key.iter().cloned().chain([i.to_string()]).collect();
                        let Some(item) = item.as_object() else { return Some(format!("Expected object\n  at {}", at(&at_item))) };
                        if let Some(d) = decode_in(nested, item, &at_item) {
                            return Some(d);
                        }
                    }
                }
            }
            Some(v) if !is(kind, v) => return Some(format!("Expected {}\n  at {}", if kind.starts_with("p(") { "string".into() } else { expected(kind) }, at(&key))),
            Some(v) if kind.starts_with("p(") && !v.as_str().is_some_and(pattern(&kind[2..kind.len() - 1]).1) => return Some(format!("Expected {}\n  at {}", expected(kind), at(&key))),
            Some(_) => {}
        }
    }
    None
}

/// `ModelSelection` of the pin: the source struct needs `model`, the instance is `instanceId` or else a string `provider`, then the wire struct
/// wants the instance and the model as strings and `options` as an array or an object.
fn model_selection(value: &Value, name: &str) -> Option<String> {
    let Some(raw) = value.as_object() else { return Some(format!("Expected object\n  at {}", at(&[name]))) };
    if !raw.contains_key("model") {
        return Some(format!("Missing key\n  at {}", at(&[name, "model"])));
    }
    let instance = raw.get("instanceId").or_else(|| raw.get("provider").filter(|p| p.is_string()));
    if !instance.is_some_and(Value::is_string) {
        return Some(format!("Expected string\n  at {}", at(&[name, "instanceId"])));
    }
    if !raw["model"].is_string() {
        return Some(format!("Expected string\n  at {}", at(&[name, "model"])));
    }
    match raw.get("options") {
        Some(o) if !o.is_array() && !o.is_object() => Some(format!("Expected array | object\n  at {}", at(&[name, "options"]))),
        _ => None,
    }
}

/// An ISO 8601 date, optionally with a time and an offset (`Z` or `+hh:mm`); the looser spellings `Date` also parses are not reproduced.
fn utc_date_time(v: &str) -> bool {
    let b = v.as_bytes();
    let num = |from: usize, len: usize| -> Option<u32> { b.get(from..from + len).filter(|d| d.iter().all(u8::is_ascii_digit)).map(|d| d.iter().fold(0, |n, c| n * 10 + u32::from(c - b'0'))) };
    let (Some(year), Some(month), Some(day)) = (num(0, 4), num(5, 2), num(8, 2)) else { return false };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => return false,
    };
    if b[4] != b'-' || b[7] != b'-' || day == 0 || day > days {
        return false;
    }
    let mut pos = 10;
    if pos < b.len() {
        if b[pos] != b'T' && b[pos] != b' ' {
            return false;
        }
        let (Some(hour), Some(minute)) = (num(11, 2), num(14, 2)) else { return false };
        if b[13] != b':' || hour > 23 || minute > 59 {
            return false;
        }
        pos = 16;
        if b.get(pos) == Some(&b':') {
            if num(pos + 1, 2).is_none_or(|s| s > 59) {
                return false;
            }
            pos += 3;
            if b.get(pos) == Some(&b'.') {
                let digits = b[pos + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
                if digits == 0 {
                    return false;
                }
                pos += 1 + digits;
            }
        }
        match &b[pos.min(b.len())..] {
            [] | [b'Z'] => {}
            [sign, ..] if (*sign == b'+' || *sign == b'-') && v.len() == pos + 6 => {
                if b[pos + 3] != b':' || num(pos + 1, 2).is_none_or(|h| h > 23) || num(pos + 4, 2).is_none_or(|m| m > 59) {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
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

    fn command(kind: &str, body: Value) -> Option<String> {
        command_decoded(kind, body.as_object().unwrap()).unwrap().err()
    }

    #[test]
    fn a_thread_command_declares_its_creation_fields_first_and_a_pull_request_key_after_the_thread() {
        // Given the fork command with only its ids, and the link command with a url and no key
        // When each is decoded, then the first declared field it lacks is the defect
        assert_eq!(command("thread.fork", json!({"commandId": "a", "threadId": "a"})).unwrap(), "Missing key\n  at [\"createdBy\"]");
        assert_eq!(command("thread.pull-request.link", json!({"commandId": "a", "threadId": "a", "url": "a", "source": "manual"})).unwrap(), "Missing key\n  at [\"host\"]");
        assert_eq!(command("thread.pull-request.watch", json!({"commandId": "a", "threadId": "a", "host": "a", "repository": "a", "number": 1})).unwrap(), "Missing key\n  at [\"watching\"]");
    }

    #[test]
    fn a_model_selection_without_an_instance_or_with_options_of_the_wrong_kind_names_the_nested_field() {
        let set = |selection: Value| command("thread.model-selection.set", json!({"commandId": "a", "threadId": "a", "modelSelection": selection}));
        // Given a selection with a model only, then the instance is the defect; a string provider stands in for it
        assert_eq!(set(json!({"model": "a"})).unwrap(), "Expected string\n  at [\"modelSelection\"][\"instanceId\"]");
        assert_eq!(set(json!({"model": "a", "provider": "codex"})), None);
        // And options that are neither an array nor an object, and a selection without a model
        assert_eq!(set(json!({"model": "a", "instanceId": "a", "options": "a"})).unwrap(), "Expected array | object\n  at [\"modelSelection\"][\"options\"]");
        assert_eq!(set(json!({"instanceId": "a"})).unwrap(), "Missing key\n  at [\"modelSelection\"][\"model\"]");
    }

    #[test]
    fn a_date_time_field_takes_iso_dates_and_refuses_other_text_as_the_utc_date_time_defect() {
        let settle = |at: Value| command("thread.settle", json!({"commandId": "a", "threadId": "a", "settledAt": at}));
        // Given settledAt as an ISO date-time, with an offset, as a date only, and as leap day
        for ok in ["2026-01-01T00:00:00Z", "2026-01-01T10:20:30.123+02:00", "2026-01-01", "2024-02-29T23:59Z"] {
            assert_eq!(settle(json!(ok)), None, "{ok}");
        }
        // When it is text that is no date, an impossible day or a number, then the field is the defect
        for bad in ["a", "2026-02-30", "2026-01-01T24:00:00Z", "2026-01-01T10:00:00+2"] {
            assert_eq!(settle(json!(bad)).unwrap(), "Expected a valid UTC DateTime string\n  at [\"settledAt\"]", "{bad}");
        }
        assert_eq!(settle(json!(7)).unwrap(), "Expected string\n  at [\"settledAt\"]");
    }

    #[test]
    fn a_keybinding_command_outside_the_union_fails_with_both_members_and_the_key_is_decoded_first() {
        let remove = |p: Value| defect("server.removeKeybinding", &p);
        // Given a key and a command that is no static name, then both union members name the command
        let d = remove(json!({"key": "a", "command": "nope"})).unwrap();
        assert!(d.starts_with("Expected \"workspace.focusNext\" | ") && d.ends_with("Expected a string matching template literal parts\n  at [\"command\"]"), "{d}");
        // And a static name and a script run command decode, a missing key comes before the command
        assert_eq!((remove(json!({"key": "a", "command": "chat.new"})), remove(json!({"key": "a", "command": "script.build-1.run"}))), (None, None));
        assert_eq!(remove(json!({"command": "nope"})).unwrap(), "Missing key\n  at [\"key\"]");
        // And the replace target of an upsert is decoded with the same union, its path nested
        assert!(defect("server.upsertKeybinding", &json!({"key": "a", "command": "chat.new", "replace": {"key": "a", "command": "x"}})).unwrap().ends_with("at [\"replace\"][\"command\"]"));
    }

    #[test]
    fn a_settings_patch_decodes_its_model_favorites_by_index_and_a_registry_limit_by_range() {
        // Given a favorite without a provider, then the defect names the item by index
        assert_eq!(defect("server.updateSettings", &json!({"patch": {"setModelFavorites": [{"favorite": true}]}})).unwrap(), "Missing key\n  at [\"patch\"][\"setModelFavorites\"][0][\"provider\"]");
        assert_eq!(defect("server.updateSettings", &json!({"patch": "wrong"})).unwrap(), "Expected object\n  at [\"patch\"]");
        assert_eq!(defect("server.updateSettings", &json!({"patch": {}})), None);
        // And a limit of zero is out of its range and a limit of 512 is not
        assert_eq!(defect("server.searchAcpRegistry", &json!({"query": "a", "limit": 0})).unwrap(), "Expected a value between 1 and 512\n  at [\"limit\"]");
        assert_eq!(defect("server.searchAcpRegistry", &json!({"query": "a", "limit": 512})), None);
    }

    #[test]
    fn a_text_generator_base_url_that_is_no_plain_http_url_is_the_filter_defect() {
        let list = |url: &str| defect("server.listTextGeneratorModels", &json!({"id": "a", "config": {"kind": "anthropic", "baseUrl": url}}));
        // Given urls that parse as plain http(s), then they decode; given text, a scheme, credentials, a query or a fragment, then the filter fails
        assert_eq!((list("https://api.example.com/v1"), list("http://a")), (None, None));
        for bad in ["a", "ftp://a", "http://u@a", "http://a?q=1", "http://a#f"] {
            assert_eq!(list(bad).unwrap(), "Expected <filter>\n  at [\"config\"][\"baseUrl\"]", "{bad}");
        }
    }

    #[test]
    fn the_orchestration_reads_decode_in_the_order_their_structs_declare_with_the_turn_range_before_the_thread() {
        let d = |tag: &str, p: Value| defect(tag, &p).unwrap();
        // Given a turn diff without its range, then the range comes first; given the range alone, then the thread is missing
        assert_eq!(d("orchestration.getTurnDiff", json!({})), "Missing key\n  at [\"fromTurnCount\"]");
        assert_eq!(d("orchestration.getTurnDiff", json!({"fromTurnCount": 0, "toTurnCount": 0})), "Missing key\n  at [\"threadId\"]");
        // And a settled cursor that is no object, a limit outside 0 to 50 and a missing thread id name their paths
        assert_eq!(d("orchestration.getOlderSettledThreads", json!({"before": "x", "limit": 0})), "Expected object\n  at [\"before\"]");
        assert_eq!(d("orchestration.getOlderSettledThreads", json!({"before": {}, "limit": 0})), "Missing key\n  at [\"before\"][\"settledAt\"]");
        assert_eq!(d("orchestration.getOlderSettledThreads", json!({"before": {"settledAt": "a"}, "limit": 51})), "Expected a value between 0 and 50\n  at [\"limit\"]");
        assert_eq!(d("orchestration.searchThread", json!({"query": "a"})), "Missing key\n  at [\"threadId\"]");
        assert_eq!(defect("orchestration.getOlderSettledThreads", &json!({"before": {"settledAt": "a"}, "limit": 50})), None);
    }

    #[test]
    fn a_launch_without_an_instance_in_its_model_selection_is_the_instance_defect_after_the_ids_and_title() {
        let launch = |model: Value| json!({"commandId": "a", "projectId": "a", "title": "a", "modelSelection": model, "runtimeMode": "auto", "interactionMode": "plan", "workspaceStrategy": {"type": "root"}});
        // Given a model selection with a model only, then the instance is the defect; given an instance too, then it decodes
        assert_eq!(defect("orchestration.launchThread", &launch(json!({"model": "a"}))).unwrap(), "Expected string\n  at [\"modelSelection\"][\"instanceId\"]");
        assert_eq!(defect("orchestration.launchThread", &launch(json!({"model": "a", "instanceId": "a", "options": "a"}))).unwrap(), "Expected array | object\n  at [\"modelSelection\"][\"options\"]");
        assert_eq!(defect("orchestration.launchThread", &launch(json!({"model": "a", "instanceId": "a"}))), None);
        assert_eq!(minimal("orchestration.launchThread").and_then(|m| defect("orchestration.launchThread", &m)), None);
    }

    #[test]
    fn the_device_payloads_name_the_first_failing_field_and_a_null_where_the_key_is_optional() {
        let d = |tag: &str, p: Value| defect(tag, &p);
        // Given a close or open without its thread, then the thread is the missing key whatever else is there
        assert_eq!(d("device.close", json!({})).unwrap(), "Missing key\n  at [\"threadId\"]");
        assert_eq!(d("device.open", json!({"deviceId": "a", "platform": "ios"})).unwrap(), "Missing key\n  at [\"threadId\"]");
        // And an optional key of the wrong type is refused with `| null`, a null passes, and the port is bounded
        assert_eq!(d("device.list", json!({"updateTool": 7})).unwrap(), "Expected \"hub\" | \"agent\" | null\n  at [\"updateTool\"]");
        assert_eq!(d("device.configure", json!({"enabled": "wrong"})).unwrap(), "Expected boolean | null\n  at [\"enabled\"]");
        assert_eq!(d("device.configure", json!({"enabled": null})), None);
        assert_eq!(d("device.testHost", json!({"id": "a", "label": "a", "target": "a", "port": 0})).unwrap(), "Expected a value between 1 and 65535\n  at [\"port\"]");
        assert_eq!(d("device.shutdown", json!({"deviceId": 7, "platform": "ios"})).unwrap(), "Expected string\n  at [\"deviceId\"]");
        // And an action naming no member fails the union, a named one is left to its handler
        assert!(d("device.action", json!("wrong")).unwrap().starts_with("Expected { readonly \"type\": \"setAppearance\", ... } | "));
        assert!(d("device.action", json!({})).is_some());
        assert_eq!(d("device.action", json!({"deviceId": "a", "type": "shake"})), None);
        assert_eq!(minimal("device.action").and_then(|m| defect("device.action", &m)), None);
        assert_eq!(minimal("device.testHost").and_then(|m| defect("device.testHost", &m)), None);
    }

    #[test]
    fn a_command_without_a_member_here_is_left_to_its_handler() {
        assert!(command_decoded("message.dispatch", &Map::new()).is_none());
    }
}
