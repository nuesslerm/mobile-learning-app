---
paths: docs/gemini-context/**, clis/knowledge/src/*.rs
name: gemini-context-sync
description: >
  Use when asked to regenerate, refresh, or sync the Gemini Gem codebase
  context, or when the Gem's answers about the repository are stale. Triggers
  on "sync the gemini docs", "regenerate the gem context", "the codebase
  context is stale".
license: MIT
metadata:
  origin: custom
  category: dev-tooling
  claude_code: supported
  opencode: supported
  compatibility: Claude Code, OpenCode
---

# Gemini Gem context sync

The Gem's knowledge is a family of Google Docs in one Drive folder: `repomix-context-00-architecture` (`ARCHITECTURE.md`: layering rules, package map, key flows) and `repomix-context-01` ... `-08` (the full repomix pack, at most 1,000k characters each; a Doc holds about 1,024,000 and a Gem takes 10 files). Each Doc is created once and overwritten in place afterwards, so the Gem's attachments never change and Drive-linked Docs refresh.

## The flow (`wf knowledge index --gemini`)

1. `pnpm exec repomix --compress --style markdown` writes a signature-only skeleton of `backend/ clis/ frontend/mla-tamagui/ infra/`.
2. `wf offload ask --prompt-file` drafts `ARCHITECTURE.md` from `docs/gemini-context/architecture-prompt.md`, `CLAUDE.md`, `CONCEPTS.md` and the skeleton (3 attempts; an empty or `WROTE_NOTHING` answer fails the run).
3. repomix packs the full source minus generated data (`PACK_IGNORE`: stroke tables, golden snapshots, the key registry, `*.generated.{ts,rs,json}` modules, `fixtures/`, `baselines/` and `contracts/data/` recordings; repomix globs with a trailing `.*` such as `*.generated.*` silently match nothing, so name the extensions), and the pack is cut at file boundaries into parts of at most 1,000k characters. More than 10 Docs fails the run.
4. Drive REST (`gem_drive.rs`, token from `gcloud auth print-access-token`): each part is found by name in the folder and overwritten (`PATCH` media), or created as a Google Doc. Each Doc is read back (mime type, exported character count within 2% of what was sent, modified within 15 minutes); one proof line per Doc goes to the job summary. A Doc of the family that no part replaced is reported as stale, never deleted. The manifest `docs/gemini-context/manifest.json` (version 3) records the Docs.

Flags: `--dry-run` builds the Docs under `.gemini-context-cache/docs/` and uploads nothing; `--no-summary` reuses the cached `ARCHITECTURE.md`;
`--remote local:<dir>/<prefix>` writes `<prefix>-NN.md` files to a directory instead of Drive.

## Running it

- In CI: `.github/workflows/repomix-gemini-context.yml` on a GitHub-hosted runner (no Go or rclone: plain Drive REST). The destination is `GEMINI_CONTEXT_REMOTE` in the workflow's `env` (the Drive folder `Gemini Gem context` plus the Doc name prefix).
  - **To refresh by hand:** the nightly and weekly sync are native schedules; the tag is only a manual run. Dispatch the workflow (`mode`), or push an *empty* commit to `dev` whose message carries `[gem-sync]` (drafts `ARCHITECTURE.md` anew) or `[gem-sync cached]` (reuses the runner's cached draft, no model call): `git commit --allow-empty -m "[gem-sync]" && git push origin dev`. The job runs only for a tagged commit that changes no file, and the CI (`deploy.yml`) does not start for a push that changes no file, so the trigger neither runs the pipeline nor displaces a waiting CI run.
  - **The clock:** the workflow's own `schedule`: `23 2 * * *` UTC runs the cached form, `37 3 * * 0` the full form. GitHub runs a schedule only from `main` and hours late, so nothing is live until `dev` is merged to `main`; the tagged empty commit stays for a manual run. Runs queue in the `gemini-context` concurrency group, one at a time; a run replaced by a newer waiting one ends `cancelled`, which is the queue working.
  - **Emptiness is read from the commits API** (`scheduled-triggers.yml`): the push event's file lists are null for every commit, so a stray tag in a commit that changes files starts nothing.
  - **Proof:** every Doc is read back and `doc <name> id=<id> chars=<n> modified=<t>` goes to the job summary; the same ids every run mean the Gem's attachments are overwritten in place. A failed draft fails the run and leaves the Drive file as it was.
- By hand: `cargo build --release -p knowledge`, then `clis/target/release/knowledge knowledge index --gemini`.
- Credential: the Drive-scoped gcloud login on agent-shell, from `wf secret login gcloud` (add `--copy` in a sandbox to also copy it there; it runs `gcloud auth login --enable-gdrive-access`). A 401 or 403 means re-login. In a Claude Code Remote sandbox the token is the literal `proxy-injected`; run on agent-shell or the VM runner instead.

## The system prompt

`docs/gemini-context/system-prompt.txt` is the Gem's instruction text: paste it into the Gem when it changes. It tells the Gem to read the ARCHITECTURE section first and to cite file paths.

## Known gaps

`docs/gaps/gemini-sync-unverified.md` (no real Drive upload yet) and `docs/gaps/gemini-gem-large-doc-grounding.md` (does the Gem ground on late files of the bundle).
