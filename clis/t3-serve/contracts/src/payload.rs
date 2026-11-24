//! The request payloads of the RPCs below, decoded before any handler runs the way the pinned server's Effect schema
//! as the pinned server's Effect schema refuses them: the first field in declaration order that is missing or of the wrong type is a defect, never a typed error.
use serde_json::{Map, Value};

/// Fields in the order the contract's struct declares them: `name:kind`, kind `s` string, `n` number, `b` boolean, `a` array, `o` object, `sn` string or null, `p(day|http|npm)` a string matching a contract pattern, `l(x|y)` one of
/// the literals, `ln(x|y)` one of the numeric literals, `pi` an integer above 0, `pp` a port (above 0 and below 65536), `vp` a preview viewport setting, `ns` a navigation status, a trailing `?` for an optional key, a trailing `~` for an optional key that also takes null (its defect names `| null`). What a field holds beyond its kind (a string's trim and length, an array's items) is the handler's.
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
    ("provider.chatgpt.import-profile", "instanceId:s profile:o(chatgptTransferred)"),
    ("provider.chatgpt.handoff.subscribe", "instanceId:s environmentId:s attemptId:s returnUrl:s profile:o(chatgptProfile)|"),
    ("provider.auth.removeMethod", "instanceId:s methodId:s"),
    ("provider.uploadFeedback", "threadId:s reason:s?"),
    ("server.updateProvider", "provider:s targetVersion:s? instanceId:s?"),
    ("projects.searchEntries", "cwd:s query:s limit:n kind:l(file|directory)? imageOnly:b?"),
    ("projects.searchContents", "cwd:s query:s limit:n caseSensitive:b wholeWord:b useRegex:b"),
    ("projects.listIgnoredFiles", "cwd:s"),
    ("projects.writeFile", "cwd:s relativePath:s contents:s"),
    ("projects.createNew", "name:s"),
    ("projects.readFile", "cwd:s relativePath:s allowMissing:b?"),
    ("agentSessions.import", "projectId:s expectedWorkspaceRoot:s?"),
    ("pullRequests.routingIdentity", "host:s"),
    ("terminal.open", "threadId:s terminalId:s? cwd:s? worktreePath:sn? cols:pi? rows:pi? env:x? providerInstanceId:s? stopOnSettle:b?"),
    ("pullRequests.filterOptions", "kind:l(labels|authors|references) projectId:s? projectIds:a? host:s? query:s? sigil:l(#|!)?"),
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
    ("guestShare.create", "threadId:s"),
    ("peerLinks.unlink", "environmentId:s"),
    ("peerLinks.link", "url:s alternateUrls:a? pairingCode:s access:l(read-only|approval-required|auto-accept-edits|auto|full-access) expectedEnvironmentId:s? expectedLabel:s?"),
    ("peerLinks.answerRequest", "threadId:s turnItemId:s answer:x"),
    ("preview.clearProfile", "profileId:s"),
    ("subscribeDiscoveredLocalServers", "configuredUrls:a~"),
    ("preview.stopLocalServer", "pid:pi port:pp"),
    ("preview.reportProfiles", "profiles:a(browserProfile) defaultProfileId:s recordingFrameRate:ln(30|60)~"),
    ("preview.open", "threadId:s url:s~ viewport:vp~ profileId:s~ runtime:l(desktop|server)~ reveal:b~"),
    ("preview.reportStatus", "threadId:s tabId:s navStatus:ns canGoBack:b canGoForward:b"),
    ("preview.resize", "threadId:s tabId:s viewport:vp"),
    ("preview.adjust", "threadId:s tabId:s colorScheme:l(system|light|dark)~ zoomFactor:ln(0.25|0.33|0.5|0.67|0.75|0.8|0.9|1|1.1|1.25|1.5|1.75|2|2.5|3|4|5)~ hardReload:b~ clear:l(cookies|cache)~"),
    ("scheduledTasks.upsert", "id:s? requireExisting:b? commandId:s? title:s prompt:s enabled:b schedule:o(upsertInterval) projectId:s threadId:s~ threadMode:l(new_thread|continue|fresh_session)? workspaceStrategy:x modelSelection:ms runtimeMode:l(approval-required|auto-accept-edits|auto|full-access) interactionMode:l(default|plan) createdBy:l(user|agent|system)? creationSource:l(web|mobile|mcp|provider|server)?"),
    ("scheduledTasks.listWebhookDeliveries", "id:s acceptAnyOutcome:b?"),
    ("scheduledTasks.getWebhookDelivery", "id:s deliveryId:s acceptAnyOutcome:b?"),
    ("attachments.delete", "attachmentId:s"),
    ("filesystem.getMetadata", "paths:a"),
    ("filesystem.browse", "partialPath:s cwd:s?"),
    ("attachments.createUploadUrl", "name:s mimeType:l(image/gif|image/jpeg|image/png|image/webp) sizeBytes:n"),
    ("assets.createUrl", "resource:ar"),
    ("assets.persistChatAttachments", "threadId:s messageId:s attachments:a(uploadImage)"),
    ("mcpApps.callTool", "threadId:s itemId:s name:s arguments:o"),
    ("mcpApps.toolInfo", "threadId:s itemId:s name:s"),
    ("mcpApps.readResource", "threadId:s itemId:s uri:s"),
    ("mcpApps.updateModelContext", "threadId:s itemId:s conversationThreadId:s content:a? structuredContent:o?"),
    ("vcs.init", "cwd:s kind:l(git|jj|unknown)?"),
    ("vcs.listRefs", "cwd:s query:s? cursor:n? includeMatchingRemoteRefs:b? refKind:l(all|local|remote)? refresh:b? limit:n?"),
    ("vcs.createRef", "cwd:s refName:s switchRef:b?"),
    ("vcs.switchRef", "cwd:s refName:s"),
    ("vcs.pull", "cwd:s"),
    ("vcs.refreshBranch", "cwd:s"),
    ("subscribeVcsBranch", "cwd:s"),
    ("subscribeWorktreeSetup", "threadId:s"),
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
    ("sourceControl.star", "provider:s host:s repository:s"),
    ("sourceControl.starred", "provider:s host:s repository:s"),
    ("sourceControl.publishRepository", "cwd:s provider:s repository:s visibility:l(private|public) remoteName:s? protocol:l(auto|ssh|https)?"),
    ("review.getBranchCommits", "cwd:s baseRef:s?"),
    ("projects.listEntries", "cwd:s directoryPath:s?"),
    ("server.refreshProviders", "instanceId:s~ cwd:s?"),
    ("pullRequests.list", "state:l(all|open|closed|merged) involvement:l(all|reviewing|authored)? filters:o? projectId:s? projectIds:a? host:s? limit:r(1,500)? cursors:o? query:s?"),
    ("pullRequests.listStats", "refs:a(pullRequestRef)"),
    ("pullRequests.reportState", "reference:o(pullRequestRef) state:l(open|closed|merged) isDraft:b?"),
    ("pullRequests.invalidate", "reference:o(pullRequestRef)~ filesViewedOnly:b?"),
    ("review.getDiffPreview", "cwd:s baseRef:s? commitSha:s? ignoreWhitespace:b? sourceKind:l(working-tree|branch-range)? trackedOnly:b? metadataOnly:b? file:o?"),
];

/// Structs a field of kind `o(name)` or `a(name)` holds, declared like `SCHEMAS`; only the fields the pinned contracts declare first are decoded for the settings patch.
const NESTED: &[(&str, &str)] = &[
    ("upsertInterval", "type:l(interval) everyMs:pi"),
    ("keybindingRule", "key:s command:kc when:s?"),
    ("textGenerator", "kind:l(openaiCompatible|anthropic) label:s? baseUrl:p(url) enabled:b?"),
    ("settingsPatch", "setModelFavorites:a(favoriteItem)? setModelsHidden:a(hiddenItem)? setProviderModelOrder:o(modelOrder)?"),
    ("browserProfile", "id:s name:s kind:l(persistent|incognito)"),
    ("uploadImage", "id:s? type:l(image) name:s mimeType:p(image) sizeBytes:n dataUrl:s source:x?"),
    ("settledCursor", "settledAt:s threadId:s?"),
    ("notification", "source:x?"),
    ("pullRequestRef", "projectId:s host:s? expectedAccountId:s? allowStale:b? repository:s number:pi"),
    ("favoriteItem", "provider:s model:s favorite:b"),
    ("hiddenItem", "provider:s model:s hidden:b"),
    ("modelOrder", "provider:s modelOrder:a"),
    ("chatgptProfile", "clientId:p(oaiapp) subject:s? email:sn? redirectUri:p(callback)? connectionLabel:s? sharingEnabled:b? idTokenHint:s?"),
    ("chatgptTransferred", "registration:o(chatgptProfile) credentials:o(chatgptCredentials)"),
    ("chatgptCredentials", "clientId:s accessToken:s refreshToken:sn idToken:s issuer:s expiresAt:n earliestRefreshAt:n| scopes:a subject:s email:sn"),
];

/// The commands of `orchestration.dispatchCommand` whose union member is decoded here, fields in the order the member declares them (the creation fields and
/// `ThreadPullRequestKey` spread in place). Beyond the kinds above: `dt` a UTC date-time string, `ms` a model selection, `x` any value that is present.
const COMMANDS: &[(&str, &str)] = &[
    ("thread.create", "createdBy:l(user|agent|system) creationSource:l(web|mobile|mcp|provider|server) commandId:s threadId:s projectId:s title:s modelSelection:ms runtimeMode:l(approval-required|auto-accept-edits|auto|full-access) interactionMode:l(default|plan) branch:sn worktreePath:sn linkOrigin:o? delegatedFrom:o? importedNativeThread:o?"),
    ("thread.guest-message.dismiss", "commandId:s threadId:s guestMessageId:s"),
    ("thread.unsettle", "commandId:s threadId:s reason:l(user)"),
    ("thread.snooze", "commandId:s threadId:s snoozedUntil:s"),
    ("thread.unsnooze", "commandId:s threadId:s reason:l(user)"),
    ("thread.auto-settle.set", "commandId:s threadId:s enabled:b"),
    ("thread.pin.reorder", "commandId:s threadId:s orderKey:s"),
    ("thread.active.reorder", "commandId:s threadId:s orderKey:s"),
    ("thread.visit", "commandId:s threadId:s visitedAt:s opened:b? searchQuery:s?"),
    ("thread.runtime-mode.set", "commandId:s threadId:s runtimeMode:l(approval-required|auto-accept-edits|auto|full-access)"),
    ("thread.interaction-mode.set", "commandId:s threadId:s interactionMode:l(default|plan)"),
    ("provider-session.detach", "commandId:s threadId:s providerSessionId:s reason:s?"),
    ("prepared-run.retry", "commandId:s threadId:s runId:s"),
    ("subagent.stop", "commandId:s threadId:s runId:s subagentId:s"),
    ("run.interrupt", "commandId:s threadId:s runId:s reason:s? holdQueue:b?"),
    ("queued-message.promote-to-steer", "commandId:s threadId:s queuedRunId:s targetRunId:s"),
    ("queued-run.start", "commandId:s threadId:s runId:s"),
    ("queued-run.cancel", "commandId:s threadId:s runId:s"),
    ("runtime-request.respond", "commandId:s threadId:s requestId:s decision:l(accept|acceptForSession|acceptAlways|decline|cancel)? feedback:s? answers:x? attachmentsByQuestionId:x?"),
    ("thread.user-input.dismiss", "commandId:s threadId:s requestId:s"),
    ("checkpoint.rollback", "restoreFiles:b? commandId:s threadId:s scopeId:s checkpointId:s"),
    ("thread.settle", "commandId:s threadId:s expectedLatestRunId:s? settledAt:dt?"),
    ("thread.auto-settle", "commandId:s threadId:s snapshotAt:dt settledAt:dt?"),
    ("thread.tag.set", "commandId:s threadId:s tag:s tagged:b"),
    ("thread.pull-request.link", "commandId:s threadId:s host:s repository:s number:n url:s source:l(manual|created|agent|stack|stack-dismissed) repositoryWebUrl:s?"),
    ("thread.pull-request.unlink", "commandId:s threadId:s host:s repository:s number:n"),
    ("thread.pull-request-link.sync", "commandId:s threadId:s host:s repository:s number:n snapshot:o stack:x"),
    ("thread.pull-request.watch", "commandId:s threadId:s host:s repository:s number:n watching:b link:o?"),
    ("thread.model-selection.set", "commandId:s threadId:s modelSelection:ms"),
    ("provider.switch", "commandId:s threadId:s modelSelection:ms"),
    ("thread.fork", "createdBy:l(user|agent|system) creationSource:l(web|mobile|mcp|provider|server) commandId:s sourceThreadId:s targetThreadId:s sourcePoint:sp title:s? sideChat:b? createdAt:dt?"),
    ("message.dispatch", "createdBy:l(user|agent|system) creationSource:l(web|mobile|mcp|provider|server) commandId:s threadId:s messageId:s text:s attachments:a? dispatchMode:x? notification:o(notification)?"),
    ("queued-run.edit", "context:x? commandId:s threadId:s runId:s text:s"),
    ("thread.merge_back", "createdBy:l(user|agent|system) creationSource:l(web|mobile|mcp|provider|server) commandId:s sourceThreadId:s targetThreadId:s sourcePoint:sp createdAt:dt?"),
    ("delegated_task.request", "createdBy:l(user|agent|system) creationSource:l(web|mobile|mcp|provider|server) commandId:s parentThreadId:s parentRunId:s parentNodeId:s task:s title:s? modelSelection:ms runtimeMode:l(approval-required|auto-accept-edits|auto|full-access) interactionMode:l(default|plan) workspaceStrategy:x? completionWake:l(always|settled_only)? createdAt:dt?"),
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
        "image" => (r"^image\/", |v| v.get(..6).is_some_and(|p| p.eq_ignore_ascii_case("image/"))),
        "day" => (r"^\d{4}-\d{2}-\d{2}$", |v| v.len() == 10 && v.bytes().enumerate().all(|(i, b)| if i == 4 || i == 7 { b == b'-' } else { b.is_ascii_digit() })),
        "url" => ("<filter>", |v| {
            let v = v.trim();
            let Some((scheme, rest)) = v.split_once("://") else { return v.is_empty() || v.len() > 2048 };
            let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
            v.len() > 2048 || (matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") && !authority.contains('@') && !authority.is_empty() && !v.contains(['?', '#']))
        }),
        "oaiapp" => (r"^oaiapp_[\w-]+$", |v| v.strip_prefix("oaiapp_").is_some_and(|r| !r.is_empty() && r.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'))),
        "callback" => (r"^http:\/\/(?:127\.0\.0\.1|localhost):[1-9]\d{0,4}\/auth\/callback$", |v| {
            let port = v.strip_prefix("http://127.0.0.1:").or_else(|| v.strip_prefix("http://localhost:")).and_then(|r| r.strip_suffix("/auth/callback"));
            port.is_some_and(|p| (1..=5).contains(&p.len()) && !p.starts_with('0') && p.bytes().all(|b| b.is_ascii_digit()))
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

const ASSET_TAGS: [&str; 8] = ["workspace-file", "media-file", "draft-workspace-file", "attachment", "tool-output-image", "project-favicon", "native-app-icon", "github-media"];
const ASSET_RESOURCE: &str = "{ readonly \"_tag\": \"workspace-file\", ... } | { readonly \"_tag\": \"media-file\", readonly \"disposition\"?: \"attachment\", ... } | { readonly \"_tag\": \"draft-workspace-file\", ... } | { readonly \"_tag\": \"attachment\", readonly \"renderIntent\"?: \"mcp-app\", ... } | { readonly \"_tag\": \"tool-output-image\", ... } | { readonly \"_tag\": \"project-favicon\", ... } | { readonly \"_tag\": \"native-app-icon\", ... } | { readonly \"_tag\": \"github-media\", ... }";
const VIEWPORT_TAGS: [&str; 3] = ["fill", "freeform", "preset"];
const NAV_TAGS: [&str; 4] = ["Idle", "Loading", "Success", "LoadFailed"];

fn tagged_union(tags: &[&str], key: &str) -> String {
    tags.iter().map(|t| format!("{{ readonly \"{key}\": \"{t}\", ... }}")).collect::<Vec<_>>().join(" | ")
}

fn tag_in(value: &Value, tags: &[&str]) -> bool {
    value.get("_tag").and_then(Value::as_str).is_some_and(|t| tags.contains(&t))
}

fn expected(kind: &str) -> String {
    match kind {
        "n" | "pi" | "pp" => "number".into(),
        "vp" => tagged_union(&VIEWPORT_TAGS, "_tag"),
        "ns" => tagged_union(&NAV_TAGS, "_tag"),
        "ar" => ASSET_RESOURCE.into(),
        k if k.starts_with("ln(") => k[3..k.len() - 1].split('|').collect::<Vec<_>>().join(" | "),
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
        "n" | "pi" | "pp" => value.is_number(),
        "vp" => tag_in(value, &VIEWPORT_TAGS),
        "ns" => tag_in(value, &NAV_TAGS),
        "ar" => tag_in(value, &ASSET_TAGS),
        k if k.starts_with("ln(") => value.as_f64().is_some_and(|n| k[3..k.len() - 1].split('|').any(|l| l.parse::<f64>().is_ok_and(|l| l == n))),
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
    if tag == "projects.mutate" {
        return Some(serde_json::json!({"type": "project.delete", "commandId": "a", "projectId": "a"}));
    }
    let (_, schema) = SCHEMAS.iter().find(|(t, _)| *t == tag)?;
    Some(minimal_of(schema))
}

fn minimal_of(schema: &str) -> Value {
    let fields = schema.split(' ').filter_map(|f| f.split_once(':')).filter(|(_, kind)| !kind.ends_with(['?', '~'])).map(|(name, kind)| {
        let value = match kind {
            k if k.ends_with('|') => Value::Null,
            "n" | "pi" | "pp" => Value::from(1),
            "vp" => serde_json::json!({"_tag": "fill"}),
            "ns" => serde_json::json!({"_tag": "Idle"}),
            "ar" => serde_json::json!({"_tag": "project-favicon", "cwd": "a"}),
            k if k.starts_with("ln(") => k[3..k.len() - 1].split('|').next().and_then(|l| l.parse::<f64>().ok()).map_or(Value::Null, Value::from),
            "b" => Value::Bool(true),
            "a" => Value::Array(Vec::new()),
            "o" => Value::Object(Default::default()),
            "sn" => Value::Null,
            "dt" => Value::from("2026-01-01T00:00:00Z"),
            "kc" => Value::from("chat.new"),
            "ms" => serde_json::json!({"instanceId": "a", "model": "a"}),
            "sp" => serde_json::json!({"type": "latest_stable"}),
            k if k.starts_with("o(") => NESTED.iter().find(|(n, _)| *n == &k[2..k.len() - 1]).map_or_else(|| Value::Object(Default::default()), |(_, nested)| minimal_of(nested)),
            k if k.starts_with("a(") => Value::Array(Vec::new()),
            k if k.starts_with("r(") => Value::from(bounds(k).0),
            "p(url)" => Value::from("http://a"),
            "p(day)" => Value::from("2026-01-01"),
            "p(http)" => Value::from("http://a"),
            "p(npm)" => Value::from("latest"),
            "p(oaiapp)" => Value::from("oaiapp_a"),
            k if k.starts_with("l(") => Value::from(k[2..k.len() - 1].split('|').next().unwrap_or("")),
            _ => Value::from("a"),
        };
        (name.to_string(), value)
    });
    Value::Object(fields.collect())
}

pub fn workspace_provider_refresh(payload: &Value) -> Option<(&str, &str)> {
    let instance = payload["instanceId"].as_str().filter(|value| !value.trim().is_empty())?;
    let cwd = payload["cwd"].as_str().filter(|value| !value.trim().is_empty())?;
    Some((instance, cwd))
}

/// The defect text when `payload` does not decode as the schema of `tag`; `None` when it does or the tag has no schema here.
pub fn defect(tag: &str, payload: &Value) -> Option<String> {
    if tag == "device.action" {
        return device_action_defect(payload);
    }
    if tag == "projects.mutate" {
        return project_mutation_defect(payload);
    }
    if tag == "provider.consumeResetCredit" {
        return consume_reset_credit_defect(payload);
    }
    if tag == "attachments.createUploadUrl" {
        return attachment_upload_defect(payload);
    }
    let (_, schema) = SCHEMAS.iter().find(|(t, _)| *t == tag)?;
    if tag == "scheduledTasks.upsert" && payload.pointer("/schedule/type").and_then(Value::as_str) != Some("interval") {
        // The schedule is a union keyed by `type`; only the interval member is decoded here
        return decode(&schema.replace("schedule:o(upsertInterval)", "schedule:x"), payload.as_object()?);
    }
    decode(schema, payload.as_object()?)
}

/// The input is a union of an instance and a usage-limit source; a payload that fits neither fails with both members' first defects.
fn consume_reset_credit_defect(payload: &Value) -> Option<String> {
    let Some(body) = payload.as_object() else { return Some("Expected object".into()) };
    let instance = decode("instanceId:s creditId:s?", body)?;
    let source = decode("sourceId:s accountId:s creditId:s", body)?;
    Some(format!("{instance}\n{source}"))
}

const UPLOAD_UNION: &str = "Expected { readonly \"type\"?: \"image\", ... } | { readonly \"type\": \"file\", ... }";

/// The input is a union of an image member (`type` optional) and a file member; the `type` picks the member whose first defect is reported.
fn attachment_upload_defect(payload: &Value) -> Option<String> {
    let Some(body) = payload.as_object() else { return Some(UPLOAD_UNION.into()) };
    match body.get("type").map(Value::as_str) {
        None | Some(Some("image")) => decode("name:s mimeType:l(image/gif|image/jpeg|image/png|image/webp) sizeBytes:n", body),
        Some(Some("file")) => decode("name:s mimeType:s sizeBytes:n", body),
        _ => Some(UPLOAD_UNION.into()),
    }
}

const DEVICE_ACTIONS: [&str; 14] = ["setAppearance", "setTextSize", "setToggle", "setLiquidGlass", "setColorFilter", "setOrientation", "setLocation", "clearLocation", "setPermission", "openUrl", "launchApp", "terminateApp", "shake", "sendPush"];

/// A device action that names none of the union's members fails the whole union; a named member's own fields are the handler's.
fn device_action_defect(payload: &Value) -> Option<String> {
    let named = payload.get("type").and_then(Value::as_str).is_some_and(|t| DEVICE_ACTIONS.contains(&t));
    let members: Vec<String> = DEVICE_ACTIONS.iter().map(|t| format!("{{ readonly \"type\": \"{t}\", ... }}")).collect();
    (!named).then(|| format!("Expected {}", members.join(" | ")))
}

const PROJECT_MUTATIONS: [&str; 3] = ["project.create", "project.update", "project.delete"];

/// A mutation that names none of the union's members fails the whole union; a named member's own fields are the handler's.
fn project_mutation_defect(payload: &Value) -> Option<String> {
    let named = payload.get("type").and_then(Value::as_str).is_some_and(|t| PROJECT_MUTATIONS.contains(&t));
    (!named).then(|| format!("Expected {}", tagged_union(&PROJECT_MUTATIONS, "type")))
}

/// How a command of `kind` decodes: `None` when its member is not decoded here, else `Err` with the defect text of the first field that fails.
pub fn command_decoded(kind: &str, body: &Map<String, Value>) -> Option<Result<(), String>> {
    let (_, schema) = COMMANDS.iter().find(|(t, _)| *t == kind)?;
    let attachments = (kind == "queued-run.edit").then(|| body.get("attachments").and_then(Value::as_array).and_then(|items| attachments_defect(items))).flatten();
    let source = (kind == "message.dispatch").then(|| notification_source_defect(body)).flatten();
    Some(decode(schema, body).or(attachments).or(source).map_or(Ok(()), Err))
}

const SOURCE_KINDS: &str = "{ readonly \"kind\": \"delegated_task\", ... } | { readonly \"kind\": \"background_task\", readonly \"work\": \"subagent\", ... } | { readonly \"kind\": \"background_command\", ... } | { readonly \"kind\": \"subagent\", ... } | { readonly \"kind\": \"command\", ... } | { readonly \"kind\": \"monitor\", ... } | { readonly \"kind\": \"background_task\", ... } | { readonly \"kind\": \"system\", ... } | object";

/// A notification's `source` is a union of tagged members and an open object, so a value that is no object fails every member.
fn notification_source_defect(body: &Map<String, Value>) -> Option<String> {
    let source = body.get("notification")?.as_object()?.get("source")?;
    (!source.is_object()).then(|| format!("Expected {SOURCE_KINDS}\n  at {}", at(&["notification", "source"])))
}

const UNKNOWN_TYPE: &str = "Expected a string matching the RegExp ^(?!(?:image|file)$)";

/// An attachment is a union keyed by `type`: the image or file member it names and the open member for other types, each reporting its first defect.
fn attachments_defect(items: &[Value]) -> Option<String> {
    items.iter().enumerate().find_map(|(i, item)| {
        let (item, index) = (item.as_object()?, i.to_string());
        let at_item = |name: &str| format!("\n  at {}", at(&["attachments", &index, name]));
        let (member, named) = match item.get("type").and_then(Value::as_str)? {
            "image" => ("id:s name:s mimeType:p(image) sizeBytes:n", true),
            "file" => ("id:s name:s mimeType:s sizeBytes:n", true),
            _ => ("id:s name:s mimeType:s sizeBytes:n", false),
        };
        let own = decode_in(member, item, &["attachments".into(), index.clone()]);
        match (named, own) {
            (true, None) => None,
            (true, Some(d)) => Some(format!("{d}\n{UNKNOWN_TYPE}{}", at_item("type"))),
            (false, d) => d,
        }
    })
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
        let (kind, tilde) = kind.strip_suffix('~').map_or((kind, false), |k| (k, true));
        let (kind, bar) = kind.strip_suffix('|').map_or((kind, false), |k| (k, true));
        let nullable = tilde || bar;
        let (kind, optional) = kind.strip_suffix('?').map_or((kind, tilde), |k| (k, true));
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
            Some(v) if kind == "sp" => {
                if let Some(d) = source_point(v, &key) {
                    return Some(d);
                }
            }
            Some(v) if kind == "kc" => {
                if !keybinding_command(v) {
                    return Some(keybinding_command_defect(&at(&key)));
                }
            }
            Some(v) if kind == "pi" || kind == "pp" => match v.as_f64() {
                None => return Some(format!("Expected number\n  at {}", at(&key))),
                Some(n) if n.fract() == 0.0 && n <= 0.0 => return Some(format!("Expected a value greater than 0\n  at {}", at(&key))),
                Some(n) if kind == "pp" && n.fract() == 0.0 && n >= 65536.0 => return Some(format!("Expected a value less than 65536\n  at {}", at(&key))),
                Some(_) => {}
            },
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

/// `OrchestrationV2ThreadForkSourcePoint`: a union on `type`, so a value with no member's `type` fails with all three members, and a member lacking its id names it.
fn source_point(value: &Value, key: &[String]) -> Option<String> {
    let id = match value.get("type").and_then(Value::as_str) {
        Some("latest_stable") => return None,
        Some("run") => "runId",
        Some("checkpoint") => "checkpointId",
        _ => return Some(format!("Expected {}\n  at {}", tagged_union(&["latest_stable", "run", "checkpoint"], "type"), at(key))),
    };
    let at_id: Vec<String> = key.iter().cloned().chain([id.to_string()]).collect();
    match value.get(id) {
        None => Some(format!("Missing key\n  at {}", at(&at_id))),
        Some(v) if !v.is_string() => Some(format!("Expected string\n  at {}", at(&at_id))),
        Some(_) => None,
    }
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
    fn a_source_control_star_without_its_provider_is_missing_the_provider() {
        assert_eq!(defect("sourceControl.star", &json!({})).unwrap(), "Missing key\n  at [\"provider\"]");
        assert_eq!(defect("sourceControl.star", &json!({"provider": "github", "host": "h", "repository": "r"})), None);
    }

    #[test]
    fn a_terminal_open_without_its_thread_is_missing_the_thread_before_any_other_field() {
        assert_eq!(defect("terminal.open", &json!({"cwd": 7})).unwrap(), "Missing key\n  at [\"threadId\"]");
        assert_eq!(defect("terminal.open", &json!({"threadId": "t1", "cwd": "a"})), None);
    }

    #[test]
    fn the_minimal_payload_of_every_schema_decodes() {
        for (tag, _) in SCHEMAS {
            let minimal = minimal(tag).unwrap();
            assert_eq!(defect(tag, &minimal), None, "{tag}: {minimal}");
        }
    }

    #[test]
    fn a_chatgpt_profile_decodes_its_client_id_pattern_first_and_the_handoff_requires_the_key_even_as_null() {
        // Given a handoff whose profile client id misses the oaiapp pattern, then one without the key, then a null profile
        let handoff = |profile: Option<Value>| {
            let mut body = json!({"instanceId": "a", "environmentId": "a", "attemptId": "a", "returnUrl": "a"});
            if let Some(p) = profile {
                body["profile"] = p;
            }
            defect("provider.chatgpt.handoff.subscribe", &body)
        };
        // Then the pattern, the missing key and the null are answered as the pinned server does
        assert_eq!(handoff(Some(json!({"clientId": "a"}))).unwrap(), "Expected a string matching the RegExp ^oaiapp_[\\w-]+$\n  at [\"profile\"][\"clientId\"]");
        assert_eq!(handoff(None).unwrap(), "Missing key\n  at [\"profile\"]");
        assert_eq!(handoff(Some(json!(7))).unwrap(), "Expected object | null\n  at [\"profile\"]");
        assert_eq!((handoff(Some(Value::Null)), handoff(Some(json!({"clientId": "oaiapp_a-1"})))), (None, None));
    }

    #[test]
    fn an_imported_profile_names_the_registration_before_the_credentials() {
        let import = |registration: Value| defect("provider.chatgpt.import-profile", &json!({"instanceId": "a", "profile": {"registration": registration, "credentials": {"clientId": "a", "accessToken": "a", "refreshToken": null, "idToken": "a", "issuer": "a", "expiresAt": 0, "earliestRefreshAt": null, "scopes": [], "subject": "a", "email": null}}}));
        assert_eq!(import(json!({"clientId": "a"})).unwrap(), "Expected a string matching the RegExp ^oaiapp_[\\w-]+$\n  at [\"profile\"][\"registration\"][\"clientId\"]");
        assert_eq!(import(json!({"clientId": "oaiapp_a", "redirectUri": "a"})).unwrap(), "Expected a string matching the RegExp ^http:\\/\\/(?:127\\.0\\.0\\.1|localhost):[1-9]\\d{0,4}\\/auth\\/callback$\n  at [\"profile\"][\"registration\"][\"redirectUri\"]");
        assert_eq!(import(json!({"clientId": "oaiapp_a", "redirectUri": "http://localhost:4567/auth/callback"})), None);
    }

    #[test]
    fn a_reset_credit_that_fits_neither_union_member_fails_with_both_and_a_non_object_is_expected_to_be_one() {
        assert_eq!(defect("provider.consumeResetCredit", &json!({})).unwrap(), "Missing key\n  at [\"instanceId\"]\nMissing key\n  at [\"sourceId\"]");
        assert_eq!(defect("provider.consumeResetCredit", &json!("wrong")).unwrap(), "Expected object");
        assert_eq!((defect("provider.consumeResetCredit", &json!({"instanceId": "a"})), defect("provider.consumeResetCredit", &json!({"sourceId": "a", "accountId": "a", "creditId": "a"}))), (None, None));
    }

    #[test]
    fn a_feedback_upload_and_a_removed_method_decode_their_first_missing_field() {
        assert_eq!(defect("provider.uploadFeedback", &json!({})).unwrap(), "Missing key\n  at [\"threadId\"]");
        assert_eq!(defect("provider.auth.removeMethod", &json!({"instanceId": 7, "methodId": "a"})).unwrap(), "Expected string\n  at [\"instanceId\"]");
        assert_eq!(command("provider.switch", json!({"commandId": "a", "threadId": "a", "modelSelection": {"model": "a"}})).unwrap(), "Expected string\n  at [\"modelSelection\"][\"instanceId\"]");
    }

    #[test]
    fn an_asset_payload_names_the_first_field_or_resource_that_does_not_decode() {
        // Given the assets schemas: a resource union of eight tagged members and an array of upload images whose mime type is an image pattern
        let image = |mime: &str| json!({"threadId": "a", "messageId": "a", "attachments": [{"type": "image", "name": "a", "mimeType": mime}]});
        // Then a missing resource is a missing key, a resource that names no member fails the whole union, and a named one is the handler's
        assert_eq!(defect("assets.createUrl", &json!({})).unwrap(), "Missing key\n  at [\"resource\"]");
        assert!(defect("assets.createUrl", &json!({"resource": "a"})).unwrap().ends_with("| { readonly \"_tag\": \"github-media\", ... }\n  at [\"resource\"]"));
        assert_eq!(defect("assets.createUrl", &json!({"resource": {"_tag": "attachment"}})), None);
        // And an attachment's mime type that is no image is the pattern defect before the size it omits, while the thread comes first
        assert_eq!(defect("assets.persistChatAttachments", &json!({"messageId": "a", "attachments": []})).unwrap(), "Missing key\n  at [\"threadId\"]");
        assert_eq!(defect("assets.persistChatAttachments", &image("a")).unwrap(), "Expected a string matching the RegExp ^image\\/\n  at [\"attachments\"][0][\"mimeType\"]");
        assert_eq!(defect("assets.persistChatAttachments", &image("IMAGE/png")).unwrap(), "Missing key\n  at [\"attachments\"][0][\"sizeBytes\"]");
    }

    #[test]
    fn an_mcp_app_payload_names_the_first_field_that_does_not_decode() {
        // Given the mcpApps schemas in the order the contract declares them
        // Then a missing threadId is the first defect, then the item, and a complete payload decodes
        assert_eq!(defect("mcpApps.callTool", &json!({})).unwrap(), "Missing key\n  at [\"threadId\"]");
        assert_eq!(defect("mcpApps.toolInfo", &json!({"threadId": "a"})).unwrap(), "Missing key\n  at [\"itemId\"]");
        assert_eq!(defect("mcpApps.updateModelContext", &json!({"threadId": "a", "itemId": "a"})).unwrap(), "Missing key\n  at [\"conversationThreadId\"]");
        assert_eq!(defect("mcpApps.readResource", &json!({"threadId": "a", "itemId": "a", "uri": "a"})), None);
    }

    #[test]
    fn a_preview_payload_names_the_first_field_that_does_not_decode() {
        // Given the preview schemas with a port, a viewport union, a nav-status union and numeric literals
        let stop = |port: Value| defect("preview.stopLocalServer", &json!({"pid": 1, "port": port}));
        // Then a port of 0 or above 65535 and a non-number are refused as the pinned server does, a port in range decodes
        assert_eq!(stop(json!(0)).unwrap(), "Expected a value greater than 0\n  at [\"port\"]");
        assert_eq!(stop(json!(65536)).unwrap(), "Expected a value less than 65536\n  at [\"port\"]");
        assert_eq!((stop(json!("a")).unwrap(), stop(json!(8080))), ("Expected number\n  at [\"port\"]".to_string(), None));
        // And a viewport that is no tagged member, a nav status likewise, and a zoom outside the steps are the union's defects
        let viewport = "Expected { readonly \"_tag\": \"fill\", ... } | { readonly \"_tag\": \"freeform\", ... } | { readonly \"_tag\": \"preset\", ... }";
        assert_eq!(defect("preview.resize", &json!({"threadId": "a", "tabId": "a", "viewport": "a"})).unwrap(), format!("{viewport}\n  at [\"viewport\"]"));
        assert_eq!(defect("preview.open", &json!({"threadId": "a", "viewport": "a"})).unwrap(), format!("{viewport} | null\n  at [\"viewport\"]"));
        assert!(defect("preview.reportStatus", &json!({"threadId": "a", "tabId": "a", "navStatus": "a", "canGoBack": true, "canGoForward": true})).unwrap().starts_with("Expected { readonly \"_tag\": \"Idle\", ... } | "));
        assert_eq!(defect("preview.adjust", &json!({"threadId": "a", "tabId": "a", "zoomFactor": "a"})).unwrap(), "Expected 0.25 | 0.33 | 0.5 | 0.67 | 0.75 | 0.8 | 0.9 | 1 | 1.1 | 1.25 | 1.5 | 1.75 | 2 | 2.5 | 3 | 4 | 5 | null\n  at [\"zoomFactor\"]");
        assert_eq!(defect("preview.adjust", &json!({"threadId": "a", "tabId": "a", "zoomFactor": 2.0, "clear": null})), None);
        // And the profiles array is checked before the default id, its rate is 30 or 60
        assert_eq!(defect("preview.reportProfiles", &json!({"defaultProfileId": "a"})).unwrap(), "Missing key\n  at [\"profiles\"]");
        assert_eq!(defect("preview.reportProfiles", &json!({"profiles": "wrong"})).unwrap(), "Expected array\n  at [\"profiles\"]");
        assert_eq!(defect("preview.reportProfiles", &json!({"profiles": [], "defaultProfileId": "a", "recordingFrameRate": "x"})).unwrap(), "Expected 30 | 60 | null\n  at [\"recordingFrameRate\"]");
        assert_eq!(defect("preview.reportProfiles", &json!({"profiles": [{"id": "a", "name": "a", "kind": "persistent"}], "defaultProfileId": "a", "recordingFrameRate": 60})), None);
        // And the discovered-servers subscription takes its configured urls as an array or null
        assert_eq!(defect("subscribeDiscoveredLocalServers", &json!({"configuredUrls": "wrong"})).unwrap(), "Expected array | null\n  at [\"configuredUrls\"]");
        assert_eq!((defect("subscribeDiscoveredLocalServers", &json!({})), defect("subscribeDiscoveredLocalServers", &json!({"configuredUrls": ["a"]}))), (None, None));
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
    fn a_fork_or_merge_back_source_point_is_one_of_the_three_members_of_the_union() {
        let with = |kind: &str, point: Value| command(kind, json!({"createdBy": "user", "creationSource": "web", "commandId": "a", "sourceThreadId": "a", "targetThreadId": "b", "sourcePoint": point}));
        let union = "Expected { readonly \"type\": \"latest_stable\", ... } | { readonly \"type\": \"run\", ... } | { readonly \"type\": \"checkpoint\", ... }\n  at [\"sourcePoint\"]";
        for kind in ["thread.fork", "thread.merge_back"] {
            // Given a source point with no member's type, When decoded, Then the whole union is the defect
            assert_eq!((with(kind, json!({})).as_deref(), with(kind, json!({"type": "other"})).as_deref()), (Some(union), Some(union)));
            // And a member without its id names the key, while the three valid points decode
            assert_eq!(with(kind, json!({"type": "run"})).as_deref(), Some("Missing key\n  at [\"sourcePoint\"][\"runId\"]"));
            assert_eq!(with(kind, json!({"type": "checkpoint", "checkpointId": 1})).as_deref(), Some("Expected string\n  at [\"sourcePoint\"][\"checkpointId\"]"));
            assert_eq!([json!({"type": "latest_stable"}), json!({"type": "run", "runId": "r"}), json!({"type": "checkpoint", "checkpointId": "c"})].map(|p| with(kind, p)), [None, None, None]);
        }
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
        // And a delegated task fails on its first field, then on a model selection without an instance
        assert_eq!(command("delegated_task.request", json!({})).unwrap(), "Missing key\n  at [\"createdBy\"]");
        assert_eq!(command("delegated_task.request", json!({"createdBy": "user", "creationSource": "web", "commandId": "a", "parentThreadId": "a", "parentRunId": "a", "parentNodeId": "a", "task": "a", "modelSelection": {"model": "a"}})).unwrap(), "Expected string\n  at [\"modelSelection\"][\"instanceId\"]");
        assert_eq!(command("thread.pull-request.link", json!({"commandId": "a", "threadId": "a", "url": "a", "source": "manual"})).unwrap(), "Missing key\n  at [\"host\"]");
        assert_eq!(command("thread.pull-request.watch", json!({"commandId": "a", "threadId": "a", "host": "a", "repository": "a", "number": 1})).unwrap(), "Missing key\n  at [\"watching\"]");
    }

    #[test]
    fn an_edited_queued_run_names_the_members_of_the_attachment_union_that_an_attachment_fails() {
        let edit = |attachment: Value| command("queued-run.edit", json!({"commandId": "a", "threadId": "a", "runId": "a", "text": "a", "attachments": [attachment]}));
        // Given an image attachment whose mime type is no image type, then the image member's defect and the open member's refusal of the type are both reported
        assert_eq!(edit(json!({"type": "image", "id": "a", "name": "a", "mimeType": "a", "sizeBytes": 1})).unwrap(), "Expected a string matching the RegExp ^image\\/\n  at [\"attachments\"][0][\"mimeType\"]\nExpected a string matching the RegExp ^(?!(?:image|file)$)\n  at [\"attachments\"][0][\"type\"]");
        // And a well-formed image or an attachment of another type decodes
        assert_eq!(edit(json!({"type": "image", "id": "a", "name": "a", "mimeType": "image/png", "sizeBytes": 1})), None);
        assert_eq!(edit(json!({"type": "audio", "id": "a", "name": "a", "mimeType": "a", "sizeBytes": 1})), None);
        // And the fields before the attachments come first
        assert_eq!(command("queued-run.edit", json!({"commandId": "a", "threadId": "a", "runId": "a"})).unwrap(), "Missing key\n  at [\"text\"]");
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
        // And a project mutation naming no member fails the union, a file read names its first missing key
        let union = "Expected { readonly \"type\": \"project.create\", ... } | { readonly \"type\": \"project.update\", ... } | { readonly \"type\": \"project.delete\", ... }";
        assert_eq!(d("projects.mutate", json!({})).unwrap(), union);
        assert_eq!(d("projects.mutate", json!("wrong")).unwrap(), union);
        assert_eq!(d("projects.mutate", json!({"type": "project.delete"})), None);
        assert_eq!(minimal("projects.mutate").and_then(|m| defect("projects.mutate", &m)), None);
        assert_eq!(d("pullRequests.filterOptions", json!({})).unwrap(), "Missing key\n  at [\"kind\"]");
        assert_eq!(d("pullRequests.filterOptions", json!({"kind": 7})).unwrap(), "Expected \"labels\" | \"authors\" | \"references\"\n  at [\"kind\"]");
        assert_eq!(d("pullRequests.filterOptions", json!({"kind": "labels"})), None);
        // And a scheduled task fails on the first of its fields, an interval of zero on the schedule
        let task = |schedule: Value| json!({"title": "a", "prompt": "a", "enabled": true, "schedule": schedule, "projectId": "a", "workspaceStrategy": {"type": "root"}, "modelSelection": {"model": "a", "instanceId": "a"}, "runtimeMode": "auto", "interactionMode": "default"});
        assert_eq!(d("scheduledTasks.upsert", json!({})).unwrap(), "Missing key\n  at [\"title\"]");
        assert_eq!(d("scheduledTasks.upsert", task(json!({"type": "interval", "everyMs": 0}))).unwrap(), "Expected a value greater than 0\n  at [\"schedule\"][\"everyMs\"]");
        assert_eq!(d("scheduledTasks.upsert", task(json!({"type": "interval", "everyMs": 60000}))), None);
        assert_eq!(minimal("scheduledTasks.upsert").and_then(|m| defect("scheduledTasks.upsert", &m)), None);
        // And an upload url asks the image member unless the type is "file", and names the union for any other type
        let upload = |p: Value| defect("attachments.createUploadUrl", &p);
        assert_eq!(upload(json!({})).unwrap(), "Missing key\n  at [\"name\"]");
        assert_eq!(upload(json!({"type": "image", "name": "a", "mimeType": "x", "sizeBytes": 0})).unwrap(), "Expected \"image/gif\" | \"image/jpeg\" | \"image/png\" | \"image/webp\"\n  at [\"mimeType\"]");
        assert_eq!(upload(json!({"type": "file", "name": "a"})).unwrap(), "Missing key\n  at [\"mimeType\"]");
        assert_eq!(upload(json!({"type": "other"})).unwrap(), UPLOAD_UNION);
        assert_eq!(upload(json!("wrong")).unwrap(), UPLOAD_UNION);
        assert_eq!(minimal("attachments.createUploadUrl").and_then(|m| defect("attachments.createUploadUrl", &m)), None);
        // And the filesystem inputs name their first missing or mistyped field
        assert_eq!(d("filesystem.getMetadata", json!({})).unwrap(), "Missing key\n  at [\"paths\"]");
        assert_eq!(d("filesystem.getMetadata", json!({"paths": "wrong"})).unwrap(), "Expected array\n  at [\"paths\"]");
        assert_eq!(d("filesystem.browse", json!({"partialPath": 7})).unwrap(), "Expected string\n  at [\"partialPath\"]");
        assert_eq!(minimal("filesystem.browse").and_then(|m| defect("filesystem.browse", &m)), None);
        assert_eq!(d("projects.readFile", json!({})).unwrap(), "Missing key\n  at [\"cwd\"]");
        assert_eq!(d("agentSessions.import", json!({})).unwrap(), "Missing key\n  at [\"projectId\"]");
        assert_eq!(d("projects.readFile", json!({"cwd": 7})).unwrap(), "Expected string\n  at [\"cwd\"]");
        // And a worktree setup subscription needs its thread
        assert_eq!(d("subscribeWorktreeSetup", json!({})).unwrap(), "Missing key\n  at [\"threadId\"]");
        assert_eq!(d("subscribeWorktreeSetup", json!({"threadId": "t"})), None);
        assert_eq!(minimal("device.testHost").and_then(|m| defect("device.testHost", &m)), None);
        // And a guest link request without a thread id names the missing key, a string id is the handler's
        assert_eq!(d("guestShare.create", json!({})).unwrap(), "Missing key\n  at [\"threadId\"]");
        assert_eq!(d("guestShare.create", json!({"threadId": "wrong"})), None);
    }

    #[test]
    fn a_command_without_a_member_here_is_left_to_its_handler() {
        assert!(command_decoded("run.start", &Map::new()).is_none());
    }

    #[test]
    fn a_message_dispatch_names_its_first_defect_as_the_real_server_did() {
        let body = |v: Value| v.as_object().cloned().unwrap();
        let decoded = |v: Value| command_decoded("message.dispatch", &body(v)).unwrap();
        // Given a dispatch with no payload, Then the creation fields are the first missing key
        assert_eq!(decoded(json!({})).unwrap_err(), "Missing key\n  at [\"createdBy\"]");
        // And a dispatch whose notification source is no object fails every member of the union
        let sent = json!({"createdBy": "user", "creationSource": "web", "commandId": "a", "threadId": "a", "messageId": "a", "text": "a", "notification": {"source": "a"}});
        assert!(decoded(sent.clone()).unwrap_err().ends_with("| object\n  at [\"notification\"][\"source\"]"));
        // And a minimal dispatch decodes
        assert_eq!(decoded(json!({"createdBy": "user", "creationSource": "web", "commandId": "a", "threadId": "a", "messageId": "a", "text": "a", "attachments": [], "dispatchMode": {"type": "defer_start"}})), Ok(()));
    }
}
