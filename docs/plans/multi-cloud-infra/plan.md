# Multi-cloud and local infrastructure

Status: proposed, 14 Nov 2026. Owner decision pending: which provider (Oracle is appealed and may return, Hetzner is the candidate, nothing is final). The plan keeps every option open.

## Why

The Oracle Always Free tenancy that ran everything was terminated on 13 Nov 2026: the VM, every bucket and the Autonomous DB went with it. The repository was built for one provider and one host. The goal is a setup where the backend is a choice made in one place, several backends can run at once (for example Oracle `main` plus a Hetzner host), and a laptop or the sandbox can be a host too.

## What is already neutral (do not change)

- Deploys go over the tailnet through one `vm-ops-api` per host (`vm-ops-api-<HOST_LABEL>`), never by IP.
- Images are on Docker Hub, built by CI; the compose files and `vm-apps.json` declare the apps; secrets are Bitwarden.
- The cargo registry is served from `registry/` on `dev` (bc283e8).
- The `BlobStore` port in `clis/blob-cache` is clean; only its callers and one adapter are Oracle-shaped.

## What is provider-bound today (the seams)

Terraform root `infra/oracle-cloud` (A1 shape, VCN, buckets, quota and budget guards, Autonomous DB); `cloud-init.sh.tftpl` (restic over rclone `oracleobjectstorage`, `bws-aarch64`); `wf vm creds|terraform|recover|state` (the `oci` CLI, `OCI_*` keys, instance name `main`); `clis/blob-cache` `OciParStore` and `oci_cli.rs`, `wf-bucket` artifacts and goldens (PAR URL shapes); and the single host `main`, hard-coded in four places (vm-ops-api allow-list `host_allowed_apps`, `HOST` in `clis/wf-vmops/src/vmops.rs`, fixed `TS_HOSTNAME`s, CI probes).

## Architecture

### 1. Hosts are data: `infra/hosts.json`

```json
{ "main":   {"kind":"cloud","provider":"oracle","arch":"arm64","reach":"tailnet","caps":["docker","dind","redroid","vnc"]},
  "hz1":    {"kind":"cloud","provider":"hetzner","arch":"amd64","reach":"tailnet","caps":["docker","dind"]},
  "local":  {"kind":"local","arch":"native","reach":"loopback","port":18080,"caps":["docker"]} }
```

- `wf` picks a host with `--host <label>` or `WF_HOST`; the default is `main`, so nothing existing breaks. `wf ship --host a,b` fans out.
- `vm-apps.json` gains per app `requires` (capabilities) and the host map; one validator refuses an app on two hosts (the 2026-08-09 Telegram-bot incident) and refuses an app on a host that lacks a capability. `KNOWN_APPS` and `host_allowed_apps` are generated from it instead of the hard-coded `"main"`.
- Every `vm-ops-api-main` literal reads the host profile (`vmops.rs`, `tailscale_up.rs`, `pushed.rs`, harness crates, the two workflows).

### 2. Providers are modules: one cloud-init, one stack per host

- `infra/modules/cloud-init` (provider-neutral): inputs `apps`, `host_label`, `arch`, `vm_apps_json`, `secret_groups_json`, the `state_repo` block (S3-shaped for every provider).
- `infra/modules/host-oracle`, `infra/modules/host-hetzner`: compute, firewall, SSH key, `user_data` from the cloud-init module.
- `infra/stacks/<host>/main.tf` picks one provider module; Terraform state is one S3 backend keyed `<host>/terraform.tfstate`.
- `infra/oracle-cloud` stays as the `main` stack until the appeal resolves; nothing is deleted.
- `wf vm creds sync --provider`, `wf vm terraform --stack`, `HostPower { reboot, reset, state }` (oci, hcloud, docker) replace the `oci`-only verbs. Oracle-only guards (always-free quota, budget function, ONS events) stay in the Oracle module.

### 3. Blob stores are a config scheme

- One value per role and plane, the scheme picks the adapter: `par+https://…` (today), `dav+https://host:port/o/` (bearer server), `s3://bucket/prefix?endpoint=…&region=…&path_style=1&conditional=0` (R2, Hetzner Object Storage, B2, AWS, MinIO), `file:///dir`.
- Read and write stay separate credentials, and write stays dev-push only (the write secret is withheld elsewhere). Tokens and S3 keys never go in the URL (error scrubbing redacts URLs).
- A comma list names the stores of a role, all of them real members: reads try them in order, writes go to every one. It is not a fallback chain. The legacy keys (`BUILD_CACHE_*_PAR_URL`, `VERIFICATION_ARTIFACTS_*`, `GOLDEN_SCREENSHOTS_*`) stay as aliases.
- A store is declared and then required, or it is not declared. There is no optional store: a declared store that is unreachable fails `ci_cache_health` as today, so a cold cache is never silent. The only other state is no store declared, which is `WF_BLOB_CACHE=off`.
- A laptop is therefore either wired in properly as a store (always on, a stable tailnet name, an ACL rule for the runners, a token) or it is a development host (the `local` profile in section 5), not a CI store.
- Adapters: generalise `par.rs` into `http.rs` with an `Auth` strategy (None = PAR, Bearer, SigV4); S3 SigV4 hand-rolled on `hmac` and `sha2` (about 150 lines, UNSIGNED-PAYLOAD over HTTPS); a `wf bucket serve --dir --listen --token-file` verb on `tiny_http` for any always-on host (a laptop only if it is wired in as one). Callers hard-coded to `OciParStore` (`wf-bucket/src/cache.rs`, `ci_preflight.rs`) take the resolved store.

### 4. Images follow the host architecture

- Native per-arch builds, merged by manifest: `deploy-build` becomes `app x arch`; amd64 builds on `ubuntu-24.04`, arm64 on the arm runner; a merge job runs `imagetools create`. No QEMU.
- The amd64 leg is opt-in: it exists only when a host with `arch: amd64` is enabled in `hosts.json`, so while the only host is arm64 CI cost is unchanged. A Mac runs arm64 images natively.
- `ci_build.rs` re-tags an unchanged image through a one-line `FROM` build with `--platform`, which flattens a multi-arch image: it becomes `imagetools create` (a manifest copy).
- Dockerfiles that hard-code an arch (`agent-shell/Dockerfile` rclone and restic `linux-arm64`) read `TARGETARCH`; cloud-init takes `arch`.
- x86-only (decided direction, owner 14 Nov): once the last arm64 host is gone, the arm64 legs and the second runner pool are removed; the same units make that a flag, not a rewrite.

### 5. A local host

- Docker with compose v2 (Colima or Docker Desktop on the Mac, plain docker in the sandbox), the same `vm-ops-api` container with the docker socket, `HOST_LABEL=local`, host paths from `${HOST_HOME}` (today `/home/ubuntu` is hard-coded), secrets by `wf secret sync-env`.
- `loopback` reach publishes `127.0.0.1:18080/18081` and `wf` speaks plain HTTP (no SOCKS); `tailnet` reach is the same sidecar as the cloud with `HOST_LABEL=local-<name>`.
- Runs: vm-ops-api, watchdog, leetcode-backend, reporting-service, remote-chat-agent with sidecars, t3-serve, orca-serve, agent-shell without droid. Does not run: redroid, Android e2e, privileged dind on some hypervisors, anything needing the Oracle DB. CI deploys only to cloud hosts (a laptop is not reachable from a hosted runner); a local host is driven from the same machine.

## Hetzner, as researched on 14 Nov (third-party sources; re-check live)

- Prices rose on 15 Jun 2026. The line that fits our load is not reliably orderable: CAX (arm64) and CX (x86) were reported "currently unavailable" through 20 Oct, and Hetzner has a "limited availability of cloud instances" notice since 26 Jun; new accounts may be restricted. CPX and CCX were orderable but cost two to four times as much.
- Monthly, before VAT, about: CAX21 4/8 GB 10.5 to 11, CAX31 8/16 GB 21 to 21.5, CX33 4/8 GB 8.5, CX43 8/16 GB 16, CPX32 4/8 GB 35.5 to 36, CPX42 8/16 GB 69.5 to 70, CCX13 2/8 GB 43. The old VM was 2 arm64 cores and 12 GB; the nearest are 4/8 and 8/16.
- Object Storage: 6.49 EUR a month with 1 TB stored and 1 TB egress, S3-compatible, presigned URLs; reported degraded in July. Cloudflare R2: free tier 10 GB, 1 M writes, 10 M reads a month, free egress. Backblaze B2: cheap but per-call fees.
- Sign-up: ID copy or a prepaid payment by the owner's own card; a custom-domain email, no VPN, a small first order; rejections come without a reason (cda-review@hetzner.com).
- Not verified: live prices, stock after 20 Oct, hcloud provider version, S3 behaviour with sccache and the Nx remote cache.
- Consequence: Hetzner cannot be the only plan. That is the case for this design: Oracle returns, Hetzner CPX, another provider or a laptop can host `main`.

## Units, in order

Each unit is 1 to 3 files, flow-tested, landed on `dev`.

1. Blob adapters (no account needed): `http.rs` with `Auth`, SigV4 against an in-process S3 double, `wf bucket serve`; the scheme resolver and the store list; callers take the resolved store; flow tests with the fake PAR server pattern and an S3 double. Then `WF_BLOB_CACHE=off` can go.
2. Host profiles: `infra/hosts.json`, the validator, generated allow-list, `--host` in `wf-vmops` and the literals.
3. Cloud-init and provider modules: extract `infra/modules/cloud-init`, `host-oracle` (a refactor of the existing code, behaviour unchanged), `stacks/main`; then `host-hetzner`; S3-shaped state restore.
4. `wf vm` verbs per provider: `creds sync --provider`, `terraform --stack`, `HostPower` adapters.
5. Images: manifest re-tag, native per-arch matrix with the opt-in amd64 leg, Dockerfile `TARGETARCH`, cloud-init `arch`.
6. Local host: `${HOST_HOME}`, loopback reach, `wf ship --host local`, a runbook.
7. Owner steps (in parallel): provider accounts and tokens in the vault; the Oracle appeal; the ACL rule if a laptop store is used.
8. Retire: only what no provider uses (the dead `OCI_*` keys if Oracle does not return).

## Risks and open questions

- Splitting apps across hosts reintroduces the double-consumer risk: the validator is not optional.
- `wf ship` fan-out semantics (per host in CI, or one host per call) need an owner decision before unit 2 lands.
- State that lived only in the old tenancy (restic backups, Terraform state, the health-reports database) is gone: the new stacks start empty. The reporting store (`ords_store`) needs a non-Oracle adapter if the data is wanted.
- A flat Object Storage fee and Hetzner's availability notice make a free R2 cache the default recommendation for the blob stores; the compute choice stays open.
