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
last_verified: 2026-11-21
links:
  - "docs/plans/rca-parity/plan.md"
  - "docs/plans/rca-offer-more/plan.md"
  - "docs/gaps/exec-hosted-fixtures-not-from-real-clis.md"
---
# The GitHub project rows rest on one recorded board and on authored doubles for the rest, answer fifteen of sixteen rows, and are narrower than the host in a few places

## What is broken

Fifteen `github.project.*` rows of Orca are served (issue #83, O4) over `hosted.project`, `hosted.pr` and `hosted.issue`, asked of an executor whose gh is signed in
(a request with no workspace is routed by account: `group-hosted/src/account.rs`). Three things are weaker than they look.

1. **Only the user-owned project actions were recorded, once.** The run 44879766250 of `GitHub projects record` (21 Nov, `gh` 2.104.0) recorded 8 interactions of
   five actions against a throwaway user board; they are the set `exec-hosted-projects` (`provenance.kind: recorded`), replayed with the authored set by `exec_hosted::testing::fixtures()`
   (the recorded exchanges of an action after its authored ones). Of the 34 interactions of `exec-hosted` for `hosted_project_*`, `hosted_comment_*`, `hosted_repo_issue_types`,
   `hosted_issue_type_set` and `hosted_issue_update`, 3 were replaced by a recording (`hosted_project_list/00`, `hosted_project_resolve/01`, `hosted_project_field_clear/00`), one
   (`hosted_project_resolve/03`, a project that is not there) was corrected to the recorded shape (authored; its `locations` column, 60, is computed from the query text, not recorded, and the executor does not read it), and 31 stay authored doubles written from GitHub's documented shapes and from what Orca's
   source reads: 14 for `hosted_project_*` (an organization-owned project, a date and a single-select field, an enterprise host, the `read:project` refusal, the refusals made before gh
   starts) and 17 for the repository-bound actions. `exec-hosted` keeps `provenance.kind: imported` with 206 interactions and says so in its note (re-sealed, its `imported_from` hashes follow the rebuilt legacy files);
   `exec-hosted-projects` holds the 8 recorded ones, 3 of which replaced an authored double and the other 5 sit beside authored ones of another owner type or value (a user's views next to an organization's, a text field and a number field of 3 next to the authored 3.5).
   The mapper's `Hosting` double answers the `project.*` methods from Orca's source, and one flow test of `orca-map-hosted` feeds the mapper the recorded replies instead. What was replayed against the
   recorded real host (`replay-orca-map-hosted`) is only what that host answered on a signed-out machine. No success shape of a project row was recorded from the Orca host.
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

Built (21 Nov): `wf mimic record-projects` and `.github/workflows/github-projects-record.yml`. The job reads the classic token `GH_PROJECTS_RECORD_TOKEN`
(scopes `project` and `read:org`, vault project ci-deploy) in memory, creates a throwaway board `mla-recording-test-<run id>` on the token's user account,
runs eight requests of five actions through the real executor with a recording gh (`hosted_project_list`, `hosted_project_resolve` three times counting
a missing project, `hosted_project_views`, `hosted_project_field_set` for a text and a number field, `hosted_project_field_clear`), and deletes the board
(`if: always()`, plus a guard that deletes `mla-recording-test-*` boards older than an hour). The result is the set `exec-hosted-projects` (`provenance.kind: recorded`,
target `gh` at the version that ran, a driver `record-projects`), uploaded as the artifact `mimic-set-exec-hosted-projects`; nothing is pushed to dev from CI.

Recorded versus authored (the artifact is merged, 21 Nov):

| What | State |
| --- | --- |
| `hosted_project_list` (a viewer's user projects, no organization project), `_resolve` (owner only, a user's project by number, a project that is not there), `_views` (a user's), `_field_set` (text, number), `_field_clear` | recorded in `exec-hosted-projects`, from a user-owned board, redacted (below) |
| `hosted_project_field_set` of date, single select, iteration; organization-owned projects (resolve, views, list of organization projects); the `read:project` refusal; an enterprise host; the refusals made before gh starts | still authored in `exec-hosted`: the board has no such field and the token is a user's |
| `hosted_comment_*`, `hosted_repo_issue_types`, `hosted_issue_type_set`, `hosted_issue_update`, pull request and issue reads | still authored: they need a repository the token may write, and the token has no `repo` scope |
| the mapper's `Hosting` double for the `project.*` methods | still authored from Orca's source (its not-found message now is the recorded one); `orca-map-hosted` also has a flow test over the recorded replies |

What differed between the recorded replies and the authored doubles (the recorded one is the truth):

- **A project that is not there** is not `data: {projectV2: null}` with exit 0 and no error (the authored double), but `gh` exit 1 with the `errors` array (`type: NOT_FOUND`, message `Could not resolve to a ProjectV2 with the number N.`) on stderr as `gh: <message>`; the executor's answer is `NOT_FOUND` with `gh: gh: Could not resolve to a ProjectV2 with the number N.` through the generic failure path, and the mapper already turns that code into `project_not_found`. The authored double `hosted_project_resolve/03` and the `Hosting` double now have that shape. The branches of `shape.rs` that answered `project not found: owner/number` for an exit-0 null project (resolve and views) cannot be reached by a real `gh`; they stay as a guard and no recording reaches them.
- **The account's projects** come with their owner as `User` and an empty organization list for an account that belongs to an organization with no project; the executor's answer matched the authored one, so no code changed.
- A number field is sent as `3.0` (a GraphQL `Float`), a text field and a cleared field and the views answered as authored; no other difference was found, every recorded exchange replays through the executor, the service (`agent_hosted`) and the replay double unchanged.

Redaction: the `hosted_project_list` reply held every project and organization the account sees. In the committed set the other board is `other test board` (number 90001, URL `https://github.com/users/nuesslerm/projects/90001`, node id `PVT_kwDOFAKEother001`) and the organization the account belongs to is `example-org` (in the stdout of `gh`; the executor's reply holds no organizations). The test board `mla-recording-test-44879766250` and its node ids are kept as recorded: the board was deleted. The login `nuesslerm` is the repository owner's public login. The set header's note lists the redacted fields and says the replay shape is unchanged, and its content hash is the one the cassette format computes over the redacted content (`wf mimic verify` is clean). The registry requirement still holds for the next recording: nothing enforces that the account holds only test boards, so redact before committing.

Still open: `viewTable` with a `hosted_project_table` action read in the same run; a recording of date, single-select and iteration fields and of an organization-owned board; the repository-bound actions.
