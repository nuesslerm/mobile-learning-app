---
status: open
area: rca-parity
paths:
  - "clis/orca-serve/map-hosted/src/project.rs"
  - "clis/rca-exec/src/hosted/**"
  - "libraries/agent-api/exec-hosted/**"
  - "libraries/agent-api/agent-api-hosted/src/project.rs"
  - "backend/remote-chat-agent/group-hosted/src/**"
opened: 2026-11-20
last_verified: 2026-11-20
links:
  - "docs/plans/rca-parity/plan.md"
  - "docs/plans/rca-offer-more/plan.md"
  - "docs/gaps/exec-hosted-fixtures-not-from-real-clis.md"
---
# The GitHub project rows rest on authored doubles, answer fifteen of sixteen rows, and are narrower than the host in a few places

## What is broken

Fifteen `github.project.*` rows of Orca are served (issue #83, O4) over `hosted.project`, `hosted.pr` and `hosted.issue`, asked of an executor whose gh is signed in
(a request with no workspace is routed by account: `group-hosted/src/account.rs`). Three things are weaker than they look.

1. **No GitHub API reply behind them was recorded.** The 34 interactions of the exec-hosted set for `hosted_project_*`, `hosted_comment_*`,
   `hosted_repo_issue_types`, `hosted_issue_type_set` and `hosted_issue_update` are authored doubles: the GraphQL and REST replies are written from GitHub's
   documented shapes and from what Orca's source reads, never recorded (`provenance.kind` of the set stays `imported`, and its `note` says so). The mapper's
   `Hosting` double answers the `project.*` methods the same way. What was replayed against the recorded real host (`replay-orca-map-hosted`) is only what that host
   answered on a signed-out machine: its sign-in request (default and non-default host), the validation answers, and `{ok: true}` for an update that changes nothing.
   No success shape of a project row was ever recorded from the host.
2. **`github.project.viewTable` stays not offered.** Its table (fields with their typed values, items with field values, the view's filter, grouping, sort and
   layout, paginated by position) needs a hosted action that reads a project view through GraphQL and rca has none; the answer would be written from Orca's source
   alone, with no recorded success shape to check it against.
3. **The served rows leave out what rca does not report.** `bodyImageUrls` (display data the host never persists), the participants and the node id of a work item
   (`workItemDetailsBySlug` answers `participants: []` and `id: ""`, which is the host's own fallback), and a state reason when closing an issue (the host's schema
   strips it). Labels and assignable users are one page of at most 200. `listAccessible` reads one page of 40 viewer projects and 20 organizations of 20 projects
   each, and `listViews` one page of 100, where the host pages on. Removing labels deletes them one by one (a label already gone is not a failure), where the host
   rewrites the whole set when it removes more than one. A pull request update by slug is two calls (edit, then state), so a failure of the second leaves the first done.
   A request with no workspace goes to the one signed-in executor; with several, a repository request is refused until it names a workspace (the account that sees a
   repository is not known), and a project request goes to one holding the `project` scope or is refused as missing it (`read:org` is not checked, and `read:project` does
   not count). Which executor listed a project is not remembered, so a project id is written through whichever executor is chosen on the next request. The sign-in probes
   run together with a 5 s limit each and are not cached.
   A field value of the wrong type for its kind is a validation error in words of the mapper's own (`Invalid text value.`), where the host would send the value on.

## Why blocked

The sign-in has to ask for the `project` and `read:org` scopes (`gh auth login --scopes=project,read:org`, now in the executor's `login_args`), and the owner has to
sign gh in once more with them; the real recording then needs a GitHub account with a Projects v2 board.

## What unblocks it

Re-consent gh on one executor through `sourceControl.signInStart`, then an owner-gated run against one test organization project that records each action's output
into the exec-hosted set (`provenance.kind: recorded`) and the project rows' success shapes into the Orca recorded set. `viewTable` follows with a
`hosted_project_table` action read in the same run.
