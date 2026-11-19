---
status: open
area: rca-parity
paths:
  - "clis/orca-serve/map-hosted/src/**"
opened: 2026-11-19
last_verified: 2026-11-19
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
