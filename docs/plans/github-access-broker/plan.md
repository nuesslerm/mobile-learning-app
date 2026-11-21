# GitHub access broker: narrow what each consumer can do, widen it from agent-shell

Status: proposed, 2026-11-02. Owner idea: one broad GitHub credential held only by an operator surface (agent-shell), every other consumer (the CI runner, reporting-service, rca, builds) behind an allowlisting broker in vm-ops-api; an allowance widens by a command. The state today: `VM_PROVISION_GITHUB_TOKEN` is one PAT on `nuesslerm/expo-hello-world-app-2` with Actions rw, Administration rw, Contents read, Metadata read (owner's screenshot, 2 Nov); it feeds vm-ops-api provisioning, the build fetch, the runner's `ACCESS_TOKEN`, and the agent containers' shared env. Another PAT, `GITHUB_TOKEN`, is rca's write credential. `CI_RUNNER_GITHUB_READ_TOKEN` is not needed: the existing PAT already covers what reporting-service reads, so point reporting-service at it now and delete that registry entry.

Produced by a four-agent design workflow (inventory, feasibility, design, adversarial review). Unverified: the live token scopes beyond the screenshot, the egress proxy rules, the myoung34 runner image's internals.


Unverified, because no network or secrets were allowed: the live PAT's actual scopes, the proxy rules beyond one doc line, and how agent-shell obtains its own `gh` auth. Landing 0 closes the last gap.

## 1. Goals
- The runner, vm-ops-api, rca and the reporter each hold the least GitHub power they need.
- A job step reading `env` finds nothing that can administer the repo. Today the runner env holds a PAT that needs Administration read+write (`infra/oracle-cloud/agent-shell/docker-compose.prod.yml:265-266`). CI jobs run repo code in that container, and `:271-275` also exposes the ops, gateway and Bitwarden tokens.
- The owner can widen an allowance from agent-shell with one command, bounded by expiry and audit.
- Nothing may route around the broker, so the credential that reaches `exec-script` is scoped per caller first.

## 2. Recommendation: combination, App as credential source plus broker as policy
- A GitHub App alone cannot do per-endpoint allowlisting. Installation tokens narrow by permission category and repository list for 1 h. They cannot say "POST registration-token but not DELETE runner".
- A broker alone still needs a long-lived credential. A PAT cannot narrow per use, is tied to a person, and has one 5000/h bucket shared by all callers.
- Combined:
  - The App's private key is the only broad secret. The broker mints a narrowed installation token per allowlisted call.
  - Each caller's allowlist maps to a permission set and repo list, so a bug in path matching is still bounded by the minted token (defence in depth).
- Rejected: giving agent-shell the broad token permanently. It runs model-generated code, and every session already inherits the shared env (`docker-compose.prod.yml:82`).
- Agent-shell gets broad power through the broker, as an `admin` caller that mints on demand.

## 3. Where the broad credential lives
- The App private key lives in the vault (registry entry added first, per CLAUDE.md Secrets). It is injected only into vm-ops-api, which runs fixed scripts and no LLM or user code.
- vm-ops-api env already has precedent for a GitHub token (`backend/vm-ops-api/rs/src/infrastructure/config.rs:45-46`, redacted at `:158`).
- The key reaches nothing else: not the sibling script containers (`backend/vm-ops-api/core/src/application/scripts/redeploy.rs:160` passes `-e` for the PAT today; do not extend that), not agent-shell, not the runner.
- Remaining PATs after migration:
  - `VM_PROVISION_GITHUB_TOKEN` shrinks to Contents:Read only, which also fixes the stale registry text (`clis/secrets/src/data/registry.json:242`).
  - Rca's `GITHUB_TOKEN` is handled in landing 7.

## 4. Caller authentication
- Today every caller presents one shared credential: the rotating HMAC token plus an Ed25519 signature (`backend/vm-ops-api/core/src/domain/auth.rs:11-12,126-155`). The legacy raw seed is accepted unless `REQUIRE_SIGNED_AUTH` is set (`:148`).
- The broker routes `/github/*` are signed-only, like the Drill route (`backend/vm-ops-api/rs/src/api/routes/mod.rs:59-61`), and sit behind the dispatcher gate (`backend/vm-ops-api/rs/src/api/dispatch.rs:42-54`).
- Per-caller identity:
  - Each consumer gets its own Ed25519 keypair. The public keys sit in vm-ops-api config, mapping key id to consumer. This replaces the single `VM_GATEWAY_REQUEST_SIGNING_PUBLIC_KEY` (`infra/oracle-cloud/vm-ops-api/docker-compose.yml:67`).
  - Consumers: `runner`, `reporter`, `rca`, `ci-human` (the wf CLI) and `admin` (agent-shell only).
- The `exec-script` bypass must close first. `POST /services/agent-shell/exec-script` runs arbitrary scripts in agent-shell with whatever it holds (`backend/vm-ops-api/core/src/application/agent_shell.rs:16,78-81,109-123`). The runner already holds the gate credential.
  - Restrict `exec-script`, `/redeploy` and `/github/admin/*` to the `admin` and `ci-human` identities.
  - Until then, any allowlist can be bypassed.
- The repo, owner and ref come from config, never from the request, as in `backend/vm-ops-api/core/src/domain/provision_ref.rs:24`.

## 5. Allowlist per consumer
Defined as (method, normalized path pattern, body shape) tuples in a committed baseline (`infra/oracle-cloud/vm-ops-api/github-allowlist.json`, new), pinned to `nuesslerm/expo-hello-world-app-2` unless noted.

| Consumer | Allowed | Minted token |
| --- | --- | --- |
| `runner` | `POST .../actions/runners/registration-token`, `POST .../remove-token` | Administration:write only, this repo. Needed at start and stop only. |
| `reporter` | `GET .../actions/runners`, `GET .../actions/runs?status=queued` (`backend/reporting-service/rs/src/infrastructure/github_runner.rs:23`) | Administration:read, Actions:read |
| `ci-human` (wf) | verify_tier3: `GET` and `PATCH pulls/{n}`, body `body` only (`clis/deploy/src/verify_tier3.rs:458,463`) | Pull requests:write |
| `rca` | Deferred. Git packfiles cannot ride the broker (64 KiB body cap, `dispatch.rs:16,82`). It uses per-job minted tokens (landing 7). | Contents and PR write, 5 repos, per mint |
| `admin` | Anything the App is installed for, with audit | Per call, the narrowest permission that fits |

- Not brokered: the Actions-issued `github.token` in workflows. It is per-job, expiring and already declared in `permissions:` blocks (`.github/workflows/deploy.yml:36-39`).
- Match rules:
  - Normalize the path: drop empty segments, reject `..` and encoded slashes.
  - Cap query and body size.
  - Never forward the incoming Authorization header.
  - Redact the token in logs.

## 6. Widening an allowance
Command, run from agent-shell (the only holder of the `admin` key):

`wf github allow <consumer> <METHOD> <path-pattern> --ttl 1h --reason "..."`

- Companion verbs: `wf github revoke <grant-id>`, `wf github grants` (JSON, stable schema in `--help`) and `wf github audit --since`.
- Temporary grants:
  - Stored in `/state/github-grants.json`. The `/state` volume survives redeploy.
  - Default TTL 1 h, max 24 h. An expired grant is dropped on read.
- Permanent change: edit the committed baseline and land on dev, so a standing widening is reviewed.
- Audit: append-only JSONL of `{ts, caller, method, path, decision, grant_id, status}` for every call, allowed or denied, plus every grant and revoke. Logging uses `Logger.*` with static event names.
- A grant can never exceed the App's installed permissions. Raising those is a human action in GitHub App settings.

## 7. Failure modes
- vm-ops-api mid-swap (self-redeploy): `/github/*` is down.
  - Clients retry with wait-for-health.
  - Runner registration is not on the critical path, because vm-ops-api ships first and registration is rare.
  - Break-glass: `admin` mints a one-shot registration token via `wf`.
- Broker unreachable at runner boot: the shim retries for a bounded time, then exits non-zero. `depends_on` covers only tailscale, so the shim must wait for broker health.
- The runner uses `RUNNER_TOKEN` (1 h registration token) instead of `ACCESS_TOKEN`, so no PAT sits in the container. Whether deregistration on stop needs a remove token is unverified; test it in landing 4.
- Allowlist bug (over-permissive match): bounded by the minted token's permission set and repo list.
- App key compromise via vm-ops-api: the severe case. Mitigations: vault rotation, App key separate from the PAT, audit log.
- Rate limits: the App gets a per-installation bucket. Add per-caller quotas and ETag caching of GETs.
- Stale grant after a crash: expiry is enforced at read time, not by a timer.

## 8. Landings (each: test first, one push to dev, CI under 20 min)
0. Find how agent-shell gets its `gh` auth (agent-shell block of `infra/oracle-cloud/agent-shell/docker-compose.prod.yml`, its entrypoint). Document it. No code.
1. Registry and docs fix: restate or split `VM_PROVISION_GITHUB_TOKEN` (`clis/secrets/src/data/registry.json:242`, `agent-shell/.env.defaults:15`). Add `GITHUB_APP_ID`, `GITHUB_APP_PRIVATE_KEY` and `GITHUB_APP_INSTALLATION_ID` entries and a vm-ops-api group in `clis/secrets/src/secrets/groups.json`. Test: the registry schema check.
2. Per-caller identity in vm-ops-api auth: key id to consumer, with `exec-script` and `/redeploy` limited to `admin` and `ci-human`. Tests: unit on `auth.rs`, and integration where a runner key gets 403 on `exec-script`.
3. Broker core: pure allowlist matcher in `core`, plus the minting port, with a fake GitHub double beside `backend/vm-ops-api/integration/src/github_raw.rs`. Route `POST /github/registration-token` only, signed-only. Tests: flow plus integration with the fake.
4. Runner shim: replace `ACCESS_TOKEN` with `RUNNER_TOKEN` from the broker (`docker-compose.prod.yml:266`). Test: CI runner flow, plus a restart check including deregistration.
5. Move the reporter's runners and runs reads onto the broker, and drop `CI_RUNNER_GITHUB_READ_TOKEN` (`clis/secrets/src/data/registry.json:654-657`).
6. Grants and audit: the `/state` store and the `wf github allow|revoke|grants|audit` verbs in a `clis/*` crate, never a script. Test: flow through the binary, expiry with a fake clock.
7. Rca git credentials: a per-job minted clone token (repo list narrowed, 1 h) replacing the long-lived token in `.git/config` (`backend/remote-chat-agent/docker-entrypoint.sh:3-10,56`). Split into its own design if awkward.
8. Remove the PAT from the runner env and narrow its GitHub scope. Verify with a CI run on the vm label.

## 9. Explicitly not done
- No proxying of git smart-HTTP or packfiles.
- No brokering of the Actions `github.token`.
- No change to the Tailscale and Bitwarden tokens.
- No replacement of the shared Funnel seed beyond signed-only `/github/*`.
- No automatic widening, and no widening beyond what the App is installed for.
- No permanent holding of the broad credential in agent-shell.
- No egress proxy changes (its rules live outside this repo and could not be inspected).

**Recommendation:** combine the App and the broker. Do landing 2 first, since without scoped `exec-script` every other control can be bypassed.

## Adversarial review of the design above (resolve before landing 3)

Six verified problems, ranked by severity. Not checked: the proxy rules, the live PAT scopes, and the myoung34 image internals.

1. HIGH. The runner's per-caller key is readable by job steps.
- Job steps run inside the runner container and see its env (infra/oracle-cloud/agent-shell/docker-compose.prod.yml:262-275, which already holds the ops token, gateway seed and BWS token).
- A `runner` Ed25519 key placed there can be read by any job and used to call POST /github/registration-token. That lets a job register a rogue runner.
- Landing 4 swaps one long-lived secret for another that mints the same thing.
- Fix: fetch RUNNER_TOKEN in a one-shot init container or sidecar that alone has the key, and hand the token over through a tmpfs file, not env. Limit `runner` to one registration-token call per boot. Drop `remove-token` from the allowlist, since nothing shows the runner needs it.

2. HIGH. Landing 2 cannot stop a runner job from using exec-script.
- The runner holds VM_OPS_API_AUTH_TOKEN and VM_GATEWAY_AUTH_SEED (docker-compose.prod.yml:271-275).
- The legacy raw token is accepted unless REQUIRE_SIGNED_AUTH is set (backend/vm-ops-api/core/src/domain/auth.rs:148).
- Rotating tokens are derived from the shared config.token (auth.rs:120-125, 150-153), so anyone holding that secret can mint valid tokens.
- Fix: in landing 2, enable REQUIRE_SIGNED_AUTH and remove both values from the runner env.
- The design must also name which identity `wf ship` uses on the runner. If it is `ci-human`, the runner is `ci-human` again.

3. HIGH. "vm-ops-api runs no user code" is false, so the App key is one hop from untrusted code.
- clis/deploy/src/build_image.rs:92-108 generates a script that does git fetch with the PAT and then builds the fetched ref. Branch Dockerfile RUN steps run in that environment (hand ship via `wf ship <app> --ref <branch>`).
- backend/vm-ops-api/core/src/application/scripts/redeploy.rs:160 passes the PAT to sibling containers via -e. The design only says not to extend that.
- Fix: the build step must hold no GitHub credential. Unset it after the fetch, or use a short-lived Contents:Read installation token. Add a flow test that the build env has no GITHUB_* or VM_PROVISION_* variable.

4. HIGH. Landing 0 is mostly answered: agent-shell already holds the PAT.
- docker-compose.prod.yml:82 puts VM_PROVISION_GITHUB_TOKEN in the x-agent-environment anchor (defined :18, merged at :191, :227, :303).
- The comment there calls it Contents:Read-only, while the runner uses the same value as an Administration:write token (:265-266). So the stale text is in the compose file too, not only clis/secrets/src/data/registry.json:242.
- Every agent container that runs model-generated code therefore already has the Administration-capable PAT. This contradicts the design's rejection of giving agent-shell a permanent broad token.
- Fix: narrow the PAT to Contents:Read before relying on the broker's `admin` caller.
- Still unknown: how `gh` itself authenticates in agent-shell. The grep found no GH_TOKEN or `gh auth` wiring in the compose file or entrypoint.

5. MEDIUM. The agent-shell `admin` key is exposed to the same code the grants are meant to constrain.
- Any code an agent runs in agent-shell can use the `admin` key. Time-limited grants with audit are self-service, not preventive.
- Fix: say so in the design. Either require out-of-band human approval for grants above read-only, or accept the risk explicitly. Cap `admin` mint permissions per call and exclude Administration:write without fresh human approval.

6. MEDIUM. `ci-human` is under-specified.
- verify_tier3 (clis/deploy/src/verify_tier3.rs:458,463) PATCHes PR bodies, and wf also runs in CI jobs on the runner.
- A `ci-human` key on the runner has problem 1. A `ci-human` key only in agent-shell is just `admin`.
- Fix: state which hosts hold each key.

Minor operability points:
- Add a 413 and log-line test for brokered paths against the 64 KiB body cap (backend/vm-ops-api/rs/src/api/dispatch.rs:16).
- The audit JSONL in /state needs rotation or a size bound in landing 6.

## Decisions the review forces

- Landing 2 also sets `REQUIRE_SIGNED_AUTH`, removes `VM_OPS_API_AUTH_TOKEN` and `VM_GATEWAY_AUTH_SEED` from the runner env, and names which identity `wf ship` uses on the runner.
- The runner fetches its registration token in a one-shot init step that alone holds the key, hands it over by tmpfs file, and may call the broker once per boot; `remove-token` leaves the allowlist.
- The build step holds no GitHub credential after the fetch (a flow test asserts no `GITHUB_*` or `VM_PROVISION_*` variable in the build env); sibling script containers stop receiving the PAT.
- `VM_PROVISION_GITHUB_TOKEN` is narrowed to Contents: Read before the broker relies on it, which also removes Administration rw from every agent container.
- Grants above read-only need out-of-band human approval, or the owner accepts that agent-shell code can self-grant.

## Owner decision, 2 Nov: GITHUB_TOKEN goes away

Landed 13 Nov: the owner deleted the vault key `GITHUB_TOKEN` and its registry entries; `VM_PROVISION_GITHUB_TOKEN` is the one GitHub credential.

`GITHUB_TOKEN` (rca's clone, commit and push credential for five repos, persisted in each clone's `.git/config`) is removed in favour of `VM_PROVISION_GITHUB_TOKEN` plus the broker: rca asks agent-shell, which holds the broad credential, to do the allowed git and `gh` operations. That is landing 7 above and it ends the plan; it cannot be done first, because today `VM_PROVISION_GITHUB_TOKEN` is scoped to one repository with Contents read and rca writes to four others. Order: (a) the broker and per-caller identity land, (b) agent-shell gains a narrow "git operation on a worktree" verb set (clone, fetch, worktree add, commit, push of a branch that is not `dev`/`main`, open a PR) in a `clis/*` crate, (c) rca calls those verbs instead of `git` with a token, (d) `GITHUB_TOKEN` and `GH_TOKEN` leave the rca compose file, the registry and the vault. Resolved by the owner, 2 Nov: worktrees live in the container where agent-shell serves, as with `t3 serve`; the rca container only hosts the models and the processing, it does not host the work. rca therefore stops cloning, committing and pushing altogether: the worktree routes (`rs/src/api/routes/worktree.rs`, `docker-entrypoint.sh`'s `gh auth setup-git`) are replaced by calls to agent-shell, which already holds the repository, the workspace volume and the broad credential. rca then needs no GitHub credential at all, and step (c) becomes "delete rca's git code" rather than "call verbs instead of git". The compose mapping of `GITHUB_TOKEN` to `VM_PROVISION_GITHUB_TOKEN` (landed 2 Nov) is the transitional state until that lands.

Interim, owner 2 Nov: until agent-shell can take rca's worktrees, the work runs on the extra workspace-host container of the rca stack (`remote-chat-agent-opencode`, which already mounts the shared `/workspace` volume where `WORKTREE_BASE_DIR` lives), and that container carries the same `VM_PROVISION_GITHUB_TOKEN` as agent-shell. The `remote-chat-agent` container (models and processing) then loses its `GITHUB_TOKEN`, and its worktree routes call the workspace host to clone, commit and push. To check before landing: the opencode-server image has `git` and `gh`, and its env is not shared with model-generated code that should not hold a push credential.
