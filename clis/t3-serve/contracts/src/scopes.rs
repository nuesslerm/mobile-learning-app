//! The 17 scope literals, the set a plain pairing grants, and the RPC-to-scope table (protocol.md 2.2, 3). Only the first 60 lines
//! of the real table were read (unknown 8): the rows below are grouped by the plan's RPC groups and the scope each group's tags
//! name there; a payload-dependent scope (settings patches, stacked git actions) is reduced to its widest row. The legacy set is
//! the seven recorded in `auth.jsonl` n=3 plus `review:write`, the legacy permission that grants nothing.

pub const ALL_SCOPES: [&str; 17] = [
    "orchestration:read",
    "orchestration:operate",
    "settings:write",
    "providers:manage",
    "environment:maintain",
    "preview:operate",
    "diagnostics:read",
    "terminal:read",
    "terminal:operate",
    "filesystem:read",
    "filesystem:write",
    "source-control:write",
    "review:write",
    "access:read",
    "access:write",
    "relay:read",
    "relay:write",
];

/// What a pairing without `--scope` grants: the narrowest set that lets a phone work (plan 3.4).
pub const DEFAULT_PAIRING_SCOPES: [&str; 6] = ["orchestration:read", "orchestration:operate", "terminal:read", "terminal:operate", "filesystem:read", "filesystem:write"];

/// The subset old clients read as `scopes`; the full list travels as `permissions`.
const LEGACY_SCOPES: [&str; 8] = ["orchestration:read", "orchestration:operate", "terminal:operate", "relay:read", "access:read", "access:write", "relay:write", "review:write"];

pub fn is_scope(name: &str) -> bool {
    ALL_SCOPES.contains(&name)
}

/// The legacy view keeps the order of the grant (recorded: `auth.jsonl` n=3 lists them in the order of `permissions`).
pub fn legacy_subset<S: AsRef<str>>(granted: &[S]) -> Vec<String> {
    granted.iter().map(AsRef::as_ref).filter(|g| LEGACY_SCOPES.contains(g)).map(str::to_string).collect()
}

/// The legacy parent a denial names as `requiredScope`: the permission itself when it is a legacy one, else the read or operate scope.
pub fn legacy_parent(permission: &str) -> &'static str {
    match LEGACY_SCOPES.iter().find(|l| **l == permission) {
        Some(legacy) => legacy,
        None if permission.ends_with(":read") => "orchestration:read",
        None => "orchestration:operate",
    }
}

const NONE: &[&str] = &[];
const READ: &[&str] = &["orchestration:read"];
const OPERATE: &[&str] = &["orchestration:operate"];
const FS_READ: &[&str] = &["filesystem:read"];
const FS_WRITE: &[&str] = &["filesystem:write"];
const TERM_READ: &[&str] = &["terminal:read"];
const TERM_OPERATE: &[&str] = &["terminal:operate"];
const SOURCE_WRITE: &[&str] = &["source-control:write"];
const SETTINGS: &[&str] = &["settings:write"];
const PROVIDERS: &[&str] = &["providers:manage"];
const MAINTAIN: &[&str] = &["environment:maintain"];
const DIAGNOSTICS: &[&str] = &["diagnostics:read"];
const PREVIEW: &[&str] = &["preview:operate"];
const ACCESS_READ: &[&str] = &["access:read"];
const RELAY_READ: &[&str] = &["relay:read"];
const RELAY_WRITE: &[&str] = &["relay:write"];

const TABLE: &[(&[&str], &[&str])] = &[
    (&["server.probe", "server.getConfig", "subscribeServerConfig", "subscribeServerLifecycle"], NONE),
    (&["subscribeAuthAccess"], ACCESS_READ),
    (
        &[
            "orchestration.subscribeShell",
            "orchestration.subscribeThread",
            "orchestration.getThreadProjection",
            "orchestration.getArchivedShellSnapshot",
            "orchestration.getThreadFamily",
            "orchestration.getWorktreeThreadIds",
            "orchestration.getOlderSettledThreads",
            "orchestration.getTurnItem",
            "assets.createUrl",
            "orchestration.searchThreads",
            "orchestration.searchThread",
            "orchestration.searchThreadStream",
            "orchestration.subscribeCommandOutput",
            "orchestration.getTurnDiff",
            "orchestration.getFullThreadDiff",
            "orchestration.getWorkflowScript",
        ],
        READ,
    ),
    (
        &["orchestration.dispatchCommand", "orchestration.launchThread", "orchestration.stopWorkflow", "orchestration.forceStopThread", "projects.mutate", "projects.createNew", "projects.createFromThread", "projects.ensureScratch", "agentSessions.import"],
        OPERATE,
    ),
    (&["projects.listEntries", "projects.listIgnoredFiles", "projects.readFile", "projects.searchEntries", "projects.searchContents", "filesystem.browse", "filesystem.getMetadata", "agentSessions.scan"], FS_READ),
    (&["projects.writeFile"], FS_WRITE),
    (&["terminal.observe", "subscribeTerminalMetadata"], TERM_READ),
    (&["terminal.open", "terminal.attach", "terminal.write", "terminal.resize", "terminal.clear", "terminal.close"], TERM_OPERATE),
    (&["vcs.refreshStatus", "vcs.listRefs", "subscribeVcsStatus", "subscribeWorktreeSetup", "review.getBranchCommits", "review.getDiffPreview", "review.getDiffFileContents", "git.resolvePullRequest", "subscribeProjectClones", "sourceControl.lookupRepository", "sourceControl.appAccess", "sourceControl.starred"], READ),
    (
        &[
            "vcs.pull",
            "vcs.createWorktree",
            "vcs.removeWorktree",
            "vcs.createRef",
            "vcs.switchRef",
            "vcs.init",
            "git.runStackedAction",
            "git.preparePullRequestThread",
            "worktreeSetup.cancel",
            "projectClone.start",
            "projectClone.cancel",
            "projectClone.retry",
            "sourceControl.cloneRepository",
            "sourceControl.publishRepository",
            "sourceControl.signInStart",
            "sourceControl.signInWait",
            "sourceControl.signInCancel",
        ],
        SOURCE_WRITE,
    ),
    (&["server.discoverSourceControl"], READ),
    (&["server.updateSettings", "server.upsertKeybinding", "server.removeKeybinding"], SETTINGS),
    (
        &[
            "server.refreshProviders",
            "server.updateProvider",
            "provider.auth.start",
            "provider.auth.complete",
            "provider.auth.respond",
            "provider.auth.cancel",
            "provider.auth.logout",
            "provider.auth.subscribe",
            "provider.install.start",
            "provider.install.cancel",
            "provider.install.subscribe",
            "provider.install.remove",
            "provider.chatgpt.reconnect-profile",
            "provider.chatgpt.import-profile",
            "provider.chatgpt.handoff.subscribe",
            "provider.consumeResetCredit",
            "provider.uploadFeedback",
            "server.searchAcpRegistry",
            "server.prepareAcpRegistryAgent",
            "server.uninstallAcpRegistryManagedBinary",
            "server.acceptAcpRegistryUrlAuth",
            "server.listAcpRegistrySessions",
            "server.importAcpRegistrySession",
            "server.deleteAcpRegistrySession",
            "server.listAcpRegistryProviders",
            "server.setAcpRegistryProvider",
            "server.disableAcpRegistryProvider",
            "server.logoutAcpRegistry",
        ],
        PROVIDERS,
    ),
    (
        &[
            "server.getUsageSummary",
            "server.refreshUsageRates",
            "server.getStorageCleanupReport",
            "server.getDatabaseStorage",
            "server.getTraceDiagnostics",
            "server.getProcessDiagnostics",
            "server.getHostResources",
            "server.getProcessResourceHistory",
            "server.getResourceTelemetryHistory",
            "server.checkOtlpEndpoint",
            "server.subscribeResourceTelemetry",
            "server.reportClientActivity",
        ],
        DIAGNOSTICS,
    ),
    (
        &[
            "server.runStorageCleanup",
            "server.retryResourceTelemetry",
            "server.signalProcess",
            "server.prepareTriage",
            "server.getNpmRelease",
            "server.updateServer",
            "server.updateServerWithProgress",
            "server.commitDesktopUpdate",
            "shell.openInEditor",
        ],
        MAINTAIN,
    ),
    (&["scheduledTasks.subscribe", "scheduledTasks.listWebhookDeliveries", "scheduledTasks.getWebhookDelivery"], READ),
    (
        &["scheduledTasks.upsert", "scheduledTasks.setEnabled", "scheduledTasks.delete", "scheduledTasks.runNow", "scheduledTasks.rotateWebhookToken", "secrets.answerRequest", "guestShare.create", "peerLinks.list", "peerLinks.link", "peerLinks.unlink", "peerLinks.answerRequest"],
        OPERATE,
    ),
    (&["mcpApps.callTool", "mcpApps.toolInfo", "mcpApps.readResource", "mcpApps.updateModelContext", "attachments.createUploadUrl", "attachments.delete", "assets.persistChatAttachments"], OPERATE),
    (
        &[
            "preview.open",
            "preview.resize",
            "preview.adjust",
            "preview.close",
            "preview.list",
            "preview.clearProfile",
            "preview.reportProfiles",
            "preview.reportStatus",
            "preview.stopLocalServer",
            "subscribePreviewEvents",
            "subscribeDiscoveredLocalServers",
            "device.configure",
            "device.list",
            "device.testHost",
            "device.easAccount",
            "device.simlockIdentity",
            "device.open",
            "device.close",
            "device.shutdown",
            "device.detail",
            "device.action",
            "subscribeDeviceState",
        ],
        PREVIEW,
    ),
    (&["cloud.getRelayClientStatus"], RELAY_READ),
    (&["cloud.installRelayClient"], RELAY_WRITE),
    // The phone's pull-request RPCs and their scopes, from `RpcAuthorization.ts` and `clientRpcPermissions.ts` of the pin.
    (
        &[
            "pullRequests.list",
            "pullRequests.listStats",
            "pullRequests.filterOptions",
            "pullRequests.summary",
            "pullRequests.routing",
            "pullRequests.routingIdentity",
            "pullRequests.stack",
            "pullRequests.linkedThreads",
            "pullRequests.detail",
            "pullRequests.preview",
            "pullRequests.checks",
            "pullRequests.activity",
            "pullRequests.reviewGuide",
            "pullRequests.threadComments",
            "pullRequests.diffFileContents",
            "pullRequests.filesViewed",
            "pullRequests.changeBreakdown",
            "pullRequests.invalidate",
            "pullRequests.reportState",
            "pullRequests.subscribeRefreshes",
            "pullRequests.reviewerCandidates",
            "pullRequests.labelCandidates",
        ],
        READ,
    ),
    (
        &[
            "pullRequests.runAction",
            "pullRequests.update",
            "pullRequests.comment",
            "pullRequests.updateComment",
            "pullRequests.submitReview",
            "pullRequests.replyToThread",
            "pullRequests.setThreadResolution",
            "pullRequests.setReaction",
            "pullRequests.setFilesViewed",
            "pullRequests.requestReviewers",
            "pullRequests.setLabels",
        ],
        SOURCE_WRITE,
    ),
    (&["pullRequests.generateReviewGuide"], OPERATE),
];

/// Every RPC tag of the scope table.
pub fn known_tags() -> Vec<&'static str> {
    TABLE.iter().flat_map(|(tags, _)| tags.iter().copied()).collect()
}

/// The scopes `tag` needs, or `None` when the tag is in no row. The session answers a tag in no row with the defect
/// `Unknown request tag: <tag>`, as the real server does (`rpc-errors.jsonl` n=1), and a known tag it does not build as unsupported.
pub fn required_scopes(tag: &str) -> Option<&'static [&'static str]> {
    TABLE.iter().find(|(tags, _)| tags.contains(&tag)).map(|(_, scopes)| *scopes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_scope_the_table_names_is_one_of_the_17() {
        // Given the table
        for (tags, scopes) in TABLE {
            // When each row is read, Then its scopes are real literals and no tag is listed twice
            assert!(scopes.iter().all(|s| is_scope(s)), "{tags:?}");
            for tag in *tags {
                assert_eq!(TABLE.iter().filter(|(t, _)| t.contains(tag)).count(), 1, "{tag}");
            }
        }
    }

    #[test]
    fn the_default_pairing_is_a_subset_of_the_scopes_and_the_legacy_view_never_exceeds_it() {
        assert!(DEFAULT_PAIRING_SCOPES.iter().all(|s| is_scope(s)));
        let legacy = legacy_subset(&DEFAULT_PAIRING_SCOPES);
        assert!(legacy.iter().all(|l| DEFAULT_PAIRING_SCOPES.contains(&l.as_str())));
        assert!(!legacy.contains(&"filesystem:write".to_string()) && !legacy.contains(&"filesystem:read".to_string()));
    }

    #[test]
    fn dispatch_needs_operate_and_a_tag_in_no_row_needs_nothing_known_even_under_a_known_prefix() {
        assert_eq!(required_scopes("orchestration.dispatchCommand"), Some(OPERATE));
        assert_eq!(required_scopes("pullRequests.comment"), Some(SOURCE_WRITE));
        assert_eq!(required_scopes("pullRequests.list"), Some(READ));
        assert_eq!(required_scopes("nonsense.method"), None);
        assert_eq!(required_scopes("pullRequests.noSuchMethod"), None);
    }
}
