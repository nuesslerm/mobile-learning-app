# Workspace root, backend cargo workspace and infra files

Date: 2026-10-08. Files that belong to no single package and changed with the remote-chat-agent port.

## Deliberate behaviour changes and architecture deviations

Per section below; the files here are build inputs, so a behaviour change here is a change in what is built or how fast. The 2026-10-08 and the backend-workspace sections change none (the images carry the same services); the deploy-profile section below changes what the pipeline's binaries are.

## Scope

- `pnpm-lock.yaml` and `pnpm-workspace.yaml`: the lockfile follows the trimmed manifest of `@app/remote-chat-agent` (16 lines added, 1,192 removed); the build allowances for `ffmpeg-static` and `tesseract.js` and the `onlyBuiltDependencies` list that held only `ffmpeg-static` are removed. `sharp` and `playwright` stay: `leetcode-auth-cookie-capture` and `browser-client` use them.
- `backend/Cargo.toml` and `backend/Cargo.lock`: the backend cargo workspace gained its second member, `remote-chat-agent/rs` (crate `rca-service`, 136 crates in its closure against 9 for `leetcode-backend`).
- `infra/oracle-cloud/agent-shell/Dockerfile` and `infra/oracle-cloud/wf-cli/Dockerfile`: one `COPY` line each, for the retired `rca-edge` crate (`docs/audits/reports/clis-rca-edge.md`).
- `scripts/build-and-push-image.sh`: a comment that used the removed `AGENT_CLIS` build argument as its example now uses `WHISPER_MODELS`, which the image still has.
- `.agents/instructions/infra/oracle-cloud.md`, with the generated `infra/oracle-cloud/AGENTS.md` and `CLAUDE.md`: the credential-policy paragraph described the Node service's gateway selection and startup gate; it now says the deployed service serves opencode alone and refuses `claude` and `codex` by name, and that opencode never holds an Anthropic credential of any kind, which `wf dispatch` enforces by removing `ANTHROPIC_API_KEY` and `ANTHROPIC_AUTH_TOKEN` from the opencode child. Edited at the source and regenerated; `sync:check` passes.
- `eslint.config.mjs`: the per-package config list names `backend/{leetcode-auth-cookie-capture,leetcode-backend}`; the five removed packages (three `-mock`, two `-contracts`) are gone with their manifests, and `pnpm-lock.yaml` is regenerated (1,392 lines removed, 16 added).
- `infra/oracle-cloud/vm-ops-api/tests/trunk-deploy.test.mjs`: the sibling-directory fixture points at `backend/remote-chat-agent-contracts/src/contracts.ts`, a file that still exists.
- `.github/workflows/deploy.yml` and `clis/wf/src/deploy.rs`: `vnc-auth-session` is a fourth leg of the deploy plan (`@cli/vnc-auth-capture` plus the `wf` inputs of the `wf-cli` image it is built from), so a push to `dev` redeploys the desktop app like the other services; the plan reads the running commit from the image tag of the app's container.
- Decision log (`docs/audits/2026-09-02-token-diet-decision-log.md`) and `docs/audits/benchmarks/` hold the numbers and the five benchmark records this migration produced.

## Before and after

| Measure | Before | After |
| --- | --- | --- |
| `pnpm-lock.yaml` | | 1,192 lines fewer |
| `@app/remote-chat-agent` production tree (`pnpm list --prod`, unique packages) | 1,955 | 1,785 (the rest is peer-installed tooling and the claude SDK, which the image no longer carries) |
| Root build allowances for removed packages | 3 | 0 |
| Backend cargo workspace members | 1 | 2 |
| Runtime numbers | not applicable | not applicable |

## Tests, by tier

- `pnpm install --frozen-lockfile --offline` accepts the lockfile and prunes `node_modules` of the removed packages; typecheck and lint of the package are clean.
- `pnpm run sync:check` passes after the instruction change.
- The `leetcode-backend` image build and the `wf` build were each checked from the exact file set their Dockerfiles copy (`docs/audits/reports/backend-leetcode-backend.md`, `clis-rca-edge.md`).
- `pnpm run audit` (`pnpm audit --audit-level=high`) was started on the trimmed tree and did not finish in this sandbox in over twenty minutes (the advisory endpoint did not answer), so it is not a tier that ran; the change only removes dependencies.

## Deviations

- None.

## Known gaps

- Root `package.json` still lists `sharp` in its hoist patterns and the workspace catalog still names `playwright` and `sharp`; both have other users.
- The data files `wf` reads (`model-ladders.json`, `model-pricing.json`) still sit under `backend/remote-chat-agent/src/`, copied by name in four Dockerfiles.

## 2026-10-09: backend workspace members for the standard-crates refactor

Scope: `backend/Cargo.toml` gains a `[workspace.dependencies]` table and the members `service-kit` and `leetcode-backend/core`; `backend/Cargo.lock` follows. Both service Dockerfiles (`backend/leetcode-backend/Dockerfile`, `backend/remote-chat-agent/Dockerfile`) copy `backend/service-kit` and `backend/leetcode-backend/core` because cargo loads every workspace member even for one `-p` package (the failure of 2026-10-08, runs 291 to 297). The leetcode-backend runtime stage no longer installs `curl`; `backend/leetcode-backend/package.json` gains `bench:service` and its unit script covers the core crate.

| Measure | Before | After |
| --- | --- | --- |
| Backend workspace members | 2 | 4 |
| Image size, leetcode-backend | 266 MB (recorded 2026-10-08) | not measured here; built by the pipeline after merge |

Tests, by tier: the services' own suites (see `backend-leetcode-backend.md`); the Docker builds are not run in this session (no Docker daemon in the sandbox), so the `COPY` lines are checked by reading `cargo metadata` needs rather than by an image build. Known gap: the image build is the first check after merge.

## 2026-10-09: the deploy profile, the runner's `wf` build and the capture image copies (dev Deploy under five minutes)

Scope: `backend/Cargo.toml` and `clis/Cargo.toml` gain `[profile.deploy]` (`inherits = "release"`, `lto = false`, `codegen-units = 16`; size optimisation, `panic = "abort"` and `strip` as in release). The pipeline builds with it: `backend/remote-chat-agent/Dockerfile` (the `wfbuilder` and `rcabuilder` stages), `backend/leetcode-backend/Dockerfile`, `infra/oracle-cloud/wf-cli/Dockerfile` and `.github/actions/install-wf/action.yml` (which also builds in a target directory beside the tool cache, `${{ runner.tool_cache }}/wf-cargo-target`, because `actions/checkout` removes `clis/target` on every job). `release` stays what local builds, the benchmarks and the black-box harnesses default to. `backend/leetcode-auth-cookie-capture/Dockerfile` copies two files of `scripts/` instead of the directory (its own report). Two lockfile updates went in with it: `clis/Cargo.lock` for the `sourcemap` crate and its twelve dependencies (see `clis-wf.md`) and `backend/Cargo.lock` for `tokio-util` 0.7.20. The image changes of the services (no `curl` in the leetcode-backend runtime stage, no `openssl` in the vnc image, the new crates' `COPY` lines) are in their packages' reports.

| Measure | Before (fat LTO) | After (deploy profile) |
| --- | --- | --- |
| Rebuild of `rca-service` after a one-line change (local, four-core x86; ratio, not the VM's seconds) | 116 s | 34 s |
| Both services from cold (same machine) | n/a | 156 s |
| `rca-service` and `leetcode-backend-rs` binaries | 21.85 MB and 4.31 MB | 25.81 MB and 7.12 MB |
| Service benchmark (`bench:service`, n=5 medians: cold start, idle resident set, `/health`, chat turn, convert, voice, WebSocket) | see `rca-ci-release-lto.json` | no change beyond noise (`rca-ci-deploy-profile.json`) |
| Dev Deploy, a Rust-only change (run 310 against 316) | 14 min 45 s | 4 min 11 s |
| Dev Deploy, all four targets changed | 7 min 06 s (run 317) | 3 min 17 s (run 37966325120) |
| Dev Deploy, a lockfile change rebuilding both service images | not measured | 3 min 17 s (run 37974249978) |
| Dev Deploy, a real change to `wf` with twelve new crates | not measured | 4 min 5 s (run 37969236740) |

Tests, by tier: integration on the deploy-profile binaries (what the pipeline ships): `remote-chat-agent` 314 of 314 and `leetcode-backend` 79 of 79, none skipped, the `wf` suite 1178 of 1178; e2e: the Deploy runs above (one run per case), and the services' health and probes on the deployed images (decision log). A cold cache fill costs about 15 min once (run 315).

### Deliberate behaviour changes

- The binaries the pipeline ships are about 4 MB (`rca-service`) and 2.8 MB (`leetcode-backend-rs`) larger than the release build, in exchange for builds that are 3 to 4 times faster; the measured behaviour (start-up, resident set, latency) did not change beyond noise.
- None in what ships or when: the legs of `wf deploy plan` already list the lockfiles and the crates as inputs of the images they build into.

### Architecture deviations

- None. The profile is a Cargo setting; no crate or layer changed.

### Known gaps

- `workflow_dispatch` with `force` answers 403 from the agent session (the GitHub integration's token), so the all-services cases were timed with pushes that touch every service's package.

## Rust layout lint (2026-10-09, owner request)

New root dev tools `@ls-lint/ls-lint` (MIT, folder layout, `.ls-lint.yml`) and `@ast-grep/cli` (MIT, import direction, `backend/arch-rules/*.yml`), run by `pnpm run arch:rust` in a pre-commit job (about 3 s, jobs 3 of 5) and in the architecture workflow. `@ast-grep/cli`'s postinstall is denied in `pnpm-workspace.yaml` because it replaces the bin shim with a binary `node` cannot run; the package falls back to its runtime binary lookup (one warning line). Both rules were shown to fail on a seeded violation. Deviations: ls-lint cannot require a directory to exist and applies rules recursively until a deeper key overrides, so the layout is a whitelist of root entries; the direction rules carry two ratchet lists of files not yet behind ports.

