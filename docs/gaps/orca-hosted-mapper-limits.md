---
status: open
area: rca-parity
paths:
  - "clis/orca-serve/map-hosted/src/**"
opened: 2026-11-19
last_verified: 2026-11-20
links:
  - "docs/plans/rca-parity/plan.md"
---
# The Orca hosted mapper serves a narrower reply than the Orca host in four places

## What is broken

`orca-map-hosted` answers `github.repoSlug` from the remotes rca lists (`pr.remotes`) and reads them with a pure transcription of the host's
URL parse, and three replies are narrower than the pinned host's: an origin on a GitHub Enterprise host answers null where the host asks
`gh auth status` for the signed-in hosts and answers the Enterprise slug; an origin through an SSH host alias (`git@github-work:org/repo`, the
usual setup for several GitHub accounts) answers null where the host resolves the alias with `ssh -G`
(`src/main/github/repositories/github-ssh-host-alias-resolution.ts`); and the stand-in remote's name order only approximates the host's
`localeCompare(name, 'en')` (lowercase first, then reverse code point), so names that differ in accents or punctuation can pick another remote.

`hostedReview.getCreationEligibility` detects the provider from the same remotes: a gitlab.com origin is `gitlab` and a github.com origin (or
stand-in) is `github`; everything else answers `unsupported`, where the host also names a GitLab host `glab` is signed in to, an authenticated
GitHub Enterprise host, and Bitbucket, Azure DevOps and Gitea remotes. Its review lookup is the branch's open pull request else the latest
(as `hostedReview.forBranch`), without the host's upstream-owner, merged-fallback and rate-limit rules.

## Why blocked

The rca agent API has no method that reports the hosts `gh` or `glab` is signed in to or that resolves an SSH host alias at the workspace host,
rca's hosted group serves only GitHub and GitLab, and the mapper has no ICU collation; each needs a new executor action (or an ICU dependency) that no recorded phone flow has asked for yet.

## What unblocks it

A recorded Orca scenario whose workspace origin is an Enterprise host or an SSH alias, then a `hosted_*` executor action that runs
`gh auth status` or `ssh -G` at the workspace host and a mapper arm that reads it.

## The work-item detail rows leave out what no rca method reports

`github.workItem`, `github.workItemByOwnerRepo`, `github.workItemDetails`, `gitlab.workItemDetails` and `gitlab.workItemByPath` are composed from `issue.view`, `pr.view` (`mr.view`), their comments, checks and files. The pinned host's answers carry more, and the mapper leaves each of these out rather than inventing it: an issue's timeline items, participants and body images; a pull request's `baseSha`, `pullRequestId`, participants, viewed marks, `latestReviews`, `reviewRequests` (rca lists users and teams together), `isCrossRepository`, auto-merge and merge-queue state; a merge request's pipeline and jobs, reviewers, approval state, `baseSha`, `startSha`, the thread of a plain note and the global id in its row id (the row id is `gitlab-<kind>-<path>-<iid>`, as the list rows have it). Without the base SHA the phone cannot open a file's diff from these rows. Unlike the host, a GitHub address on an Enterprise host answers null, a pasted GitHub repository must be the workspace's upstream or origin (a preview any repository once a github.com remote exists), and a GitLab host must be gitlab.com or a host of one of the workspace's remotes.

## The CI rows leave out what rca has no action for (20 Nov, issue #83 O3)

`github.rerunPRChecks`, `github.setPRAutoMerge` and `github.prCheckDetails` are served over `pr.rerun`, `pr.setAutoMerge` and `pr.checkDetails` (and `pr.checks` to find the runs of a rerun). Where the host does more, the mapper does less rather than invent it:

- **Rerun** starts the Actions runs a pull request's checks belong to (the run id is read from each check's address, `/actions/runs/<id>/job/<job>`), the failed ones with `failedOnly`. A check from another CI, or one whose address names no run, is not rerun here: the host re-requests such a check run by its id, and rca's check rows carry no id, so a pull request with only those answers the host's `github.noRerunnableChecks` (`noFailedChecks` with `failedOnly`). The host reads the checks without its cache, rca's `pr.checks` is `gh pr checks`.
- **Auto-merge** is asked for through GitHub's `enablePullRequestAutoMerge` mutation, as the host does for a repository with no merge queue, so GitHub refuses a pull request that can already merge (`github.autoMergeAlreadyMergeable`) and nothing is merged at once. The host's other two paths are not built: a repository whose base branch requires a merge queue (the host enqueues, or runs `gh pr merge --auto`) answers GitHub's own refusal as `github.autoMergeEnableFailed`, and a stacked pull request, which the host refuses with `github.stackAutoMergeUnsupported`, is sent to GitHub. Turning it off is `gh pr merge --disable-auto`, as the host does.
- **Check details** answers the check run's output, annotations (the first 20) and the jobs of its workflow run with their steps. The workflow run is the request's `workflowRunId` or the one its `url` names; the host also reads it from the check run's `check_suite`, which rca does not. A job's `logTail` is null (the host downloads and cuts the log of a failed job), a token that cannot read check runs gets no fallback to the job read, and a read of annotations or jobs that fails leaves them empty, as the host does. The three rows answer the host's unresolved repository (`github.repoUnresolved`, null for details) only for a workspace with no remote; a remote on a host `gh` does not know is `gh`'s refusal worded as the host words it.
- **GitLab** has no Orca row over these. `mr.rerun` retries the pipeline's failed jobs (GitLab has no rerun of the passed ones, so `failed_only` is always so there), `mr.checkDetails` reads a job and the last 16 KiB of its trace when the whole trace fits the output cap (a longer trace gives no log tail, and GitHub jobs never have one) (and, with `run`, the pipeline's jobs), and `mr.setAutoMerge` sets or cancels merge when the pipeline succeeds, which merges at once when the pipeline has already passed (GitLab's own rule; nothing in its API asks for the merge to wait when it could happen now).
- T3's `enable-auto-merge` action stays `pr.merge` with `auto` (the pinned T3 host merges a clean pull request at once, like `gh pr merge --auto`); only the Orca row uses `pr.setAutoMerge`, whose refusal of a clean pull request is Orca's own.
- The 21 interactions of `exec-hosted`'s cassette for these actions (rerun, auto-merge, check details) are synthetic doubles written by hand with the fixtures, not recordings of a real gh or glab; their replay shows only that the executor shapes that invented output as the contract says.
