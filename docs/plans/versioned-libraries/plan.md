# Versioned shared libraries on blob storage: update a library without deploying every service

## Why

Each Rust service is its own Cargo workspace (manifest, lockfile, image hash of its own), but the shared crates are still path dependencies. A Dockerfile copies `backend/service-kit` and `clis/telemetry` (and rca copies `libraries/rca-core`), so one edit to `service-kit` changes every image hash and redeploys all six services (about 28 minutes of serial legs). Versioned libraries let a change ship as a new version that each service adopts when it is ready; a service that did not bump keeps its hash and does not redeploy.

## The shared pieces today

| Library | Kind | Consumers |
|---|---|---|
| `backend/service-kit` | Rust crate | all six services |
| `clis/telemetry` | Rust crate | `service-kit`, `vm-ops-api`, the `wf` crates |
| `libraries/rca-core` (+ `plugins/ladder`) | Rust crates, own workspace | `remote-chat-agent` only |
| `@lib/*` (logging, app-di, app-config, tamagui-design-system, ...) | npm, `workspace:*` | the app (one EAS Update), tooling |

The npm libraries have one deployable consumer (the app), so a change to them already ships only the app: version them only when a second deployable consumes one. The Rust crates are the real win.

## Decision (yours)

Store versioned libraries in blob storage (the OCI Object Storage the build cache already uses), no registry server, nothing paid, with old unused versions cleaned up. Built so crates.io and public npm are a later swap, not a rewrite.

## Design

1. **Rust: a static sparse registry in a bucket.** Cargo's sparse registry protocol is plain files: `config.json`, one index file per crate, and the `.crate` downloads. `wf lib release <crate> <major|minor|patch>` packages the crate (`cargo package`), uploads `crates/<name>/<version>.crate` and rewrites that crate's index file in the bucket. Consumers declare `[registries.mla] index = "sparse+<read URL>"` in their workspace's `.cargo/config.toml` and depend by version: `service-kit = { version = "1.2.0", registry = "mla" }`. The read URL is a pre-authenticated, read-only prefix URL (a PAR on `libs/`), supplied to builds as a BuildKit secret/build arg from the vault: never committed, never in an image layer. Switching to crates.io is deleting `registry = "mla"` from the dependency lines.
2. **npm: tarballs from the same bucket, no server.** `wf lib release <package>` runs `pnpm pack`, uploads `npm/<name>/<name>-<version>.tgz`, and consumers use the tarball URL with the integrity hash recorded in `pnpm-lock.yaml`. Only for a library a second deployable consumes; today none does, so this ships as the verb and a flow test, with no migration.
3. **One place names the libraries.** `libraries.json` (data): `{ name, kind: rust|npm, path, registry: mla|crates-io|npm }`. `wf lib` owns everything here (no scripts, CLAUDE.md). `wf lib retarget <name> <registry>` rewrites the dependency line in each consumer's manifest.
4. **Reuse the build cache's storage code.** `clis/blob-cache` already has the store port, the OCI adapter, PAR handling, the request and byte budget and the in-memory double; `wf lib` is a new crate on top of it with a `libs/` prefix of its own (a separate prefix from `nx/` and `sccache/`, so cache expiry never touches a published version, and the cache's 14-day expiry does not apply).
5. **Dockerfiles stop copying library source.** Cargo downloads the pinned version during `cargo chef cook`; the lock entry (name, version, checksum) is what the image hash already reads, so a service's hash changes only when it bumps.
6. **Cleanup.** `wf lib prune` deletes a published version when no service lockfile at `origin/dev` pins it, it is not the latest of its crate, and it is older than a grace period (default 30 days); it prints what it would delete with `--dry-run` and never deletes the latest or anything a lockfile on dev names. Run weekly from the existing audit workflow.
7. **Release safety.** `cargo-semver-checks` runs in `wf lib release`: a breaking change must be a major. Versions are immutable: a bucket write refuses to replace an existing `.crate` (`if_absent`), a fix is a new version.
8. **Early warning and drift.** `wf lib canary` builds and tests every consumer against the library's working tree through a `[patch.mla]` override (sandbox, Nx-cached) so a breaking change shows before any release; `wf lib status` lists, per library, the version each service pins and how far behind latest it is.

## Switching registries is a first-class requirement

Any popular registry must be a configuration change, never a code or manifest rewrite:

- **Registries are data.** `libraries.json` has a `registries` table: `{ alias, kind, url, tokenEnv, publish }` with `kind` one of `cargo-sparse` (our bucket, Kellnr, Cloudsmith, JFrog, any sparse registry), `crates-io`, `npm` (npmjs, GitHub Packages, Verdaccio, any npm-compatible registry) or `npm-tarball` (our bucket). Each library names the alias it publishes to and consumers resolve from. The token is read from the named environment variable (filled from the vault), never stored.
- **Publishing sits behind a port.** `wf lib release` calls a `Publisher` port with adapters: `BucketSparse` (ours: package, upload, write the index), `CargoPublish` (`cargo publish --registry <alias>`, which covers crates.io and every registry with the cargo publish API), `NpmPublish` (`pnpm publish --registry <url>`), `NpmTarball` (ours). A new registry kind is one adapter; the verbs, the lockfile reading, `status` and `prune` do not change (`prune` only deletes in registries whose adapter supports deletion, and skips the rest with a note, since crates.io cannot delete).
- **Consumers name an alias, not a URL.** Dependency lines say `registry = "<alias>"` (Rust) or resolve through `.npmrc` scope mapping (npm); `wf lib retarget <library> <alias>` rewrites the alias and regenerates the generated `.cargo/config.toml` `[registries]` and `.npmrc` entries from `libraries.json`. Moving to crates.io is `registry = "crates-io"` (the dependency line then drops `registry =`).
- **Proven, not assumed.** The retarget rehearsal (order step 4) runs the same release and the same consumer build against two different registry kinds (the bucket sparse registry and a local Kellnr or `cargo-http-registry` standing in for a hosted one), and the flow tests cover every adapter against a double.

## Order and verification

Spike first (changes nothing that ships), then one library at a time, each through the gate and CI:

1. **Spike on `rca-core` + `rca-ladder`**: one consumer, already its own workspace. Release both to a scratch prefix, point rca's workspace at the sparse registry, build the image from it. Record Docker build seconds cold and warm and peak memory before and after (n=1; n=3 only if a number is close). Pass line: no slower, no more memory, same behavior.
2. **`telemetry`, then `service-kit`**: release 1.0.0 then 1.0.1 with a trivial change and bump one service only. Pass line: only that service's `deploy (<service>)` leg runs, CI under 10 minutes (hard cap 20), the other five images keep their `nx-<hash>` tags (`wf ship --changed` lists one service).
3. **`canary`, `status` and `prune`** into the pre-push gate and the weekly audit; flow tests drive the real verbs against the in-memory store double.
4. **Retarget rehearsal**: `wf lib retarget` to crates.io's sparse index shape against a local static server, so the later move is proven rather than assumed.

## Risks

- **Private read access in builds.** The PAR read URL must reach the Docker build without landing in a layer (BuildKit secret); the spike proves it. PARs expire: `wf lib release` mints a long-lived read PAR and `wf secret` stores it.
- **A lockfile records the registry's index URL.** Cargo writes `source = "sparse+<index url>"` into `Cargo.lock`; if that URL is a PAR it would commit the token. Resolve before migrating any service: serve the index from a stable non-secret URL (a public-read `libs/index/` prefix is enough for the index, `.crate` downloads stay behind the PAR) or a small read proxy. `wf lib status` never prints a source URL. Built: `wf lib` (release, status, prune, retarget) with the bucket and `cargo publish` adapters; not yet: `canary`, `cargo-semver-checks`, dependency-order refusal, the npm adapters, and `libraries.json` aliases for a second registry beyond crates.io.
- **Cargo and a registry that serves static files only.** `cargo package`/`publish` talk to an API; we only use `cargo package` and write the index ourselves (the format is small and checked by a flow test that runs real `cargo` against a local file server).
- **Version skew.** Two services on different versions of a library compile it twice (the price of independence); `wf lib status` keeps it visible.
- **Library depending on a library** (`service-kit` on `telemetry`): releases go in dependency order; `wf lib release` refuses a crate whose own library dependencies are unreleased paths.
- **Storage budget.** The bucket cap (8 GB, shared) is far above a few hundred small crates; `prune` keeps it so.

## Private registry via a tailnet read proxy

**Problem.** A sparse index's `config.json` must carry the crate download link (`dl`). Today `dl` is built from the read PAR (`clis/lib/src/domain/index.rs`, `BucketSparse`) and `config.json` sits in the public `libs-index` bucket, so anyone can read the PAR and download the private crates. Cargo also records the index URL in every `Cargo.lock`, so the index URL must be non-secret too.

**Recommended design (owner has not answered; this is the default).** A small read-only proxy on the VM, tailnet only, holds the read PAR itself. Nothing public lists a crate link.

- One private bucket holds everything: crates at `libs/crates/<name>/<version>.crate` (as today) and the index at `libs/index/config.json` and `libs/index/<sparse path>`. The public `mobile-learning-libs-index` bucket and `LIBS_INDEX_WRITE_PAR_URL` are no longer needed (delete them after the migration; keep `LIBS_READ_PAR_URL` for the proxy, `LIBS_WRITE_PAR_URL` for `wf lib`).
- The proxy answers `GET /index/*` and `GET /dl/<crate>/<version>.crate` (plus `/health`) by reading the matching `libs/` object through the PAR, passing `ETag`/`If-None-Match` through (Cargo revalidates index files) and streaming the body. Anything else is 404, no listing, no write verbs.
- `config.json` is `{"dl": "http://<proxy tailnet name>:<port>/dl/{crate}/{version}.crate"}`. Lockfiles record `sparse+http://<proxy tailnet name>:<port>/index/`, a non-secret URL. `libraries.json` gives the `mla` registry that value as its public `indexUrl` (`usecases/retarget.rs` already prefers a public `indexUrl` over a secret `url`, so retarget needs no change). Plain http is fine: the tailnet is WireGuard-encrypted and each crate is checked against the checksum in the lockfile.
- Authorization is the tailnet ACL (the build's `tag:mobile-learning` reaches the proxy). A Cargo token (`auth-required: true` in `config.json`, `CARGO_REGISTRIES_MLA_TOKEN` or a BuildKit secret `env=`) is a second step, not in the first cut: it adds a secret to every build for no gain while the proxy is tailnet-only.

### Evidence: can the image builds reach it

- Builds run through `wf ship` -> `clis/deploy/src/build_image.rs`, which emits a script run in agent-shell's dind (`docker buildx create --name agent-shell-builder --driver docker-container`, line 120, **no** `network=host`, so the BuildKit container sits on dockerd's default bridge inside the dind).
- The dind shares the tailscale sidecar's network namespace (`infra/oracle-cloud/agent-shell/docker-compose.prod.yml`: `agent-dind` has `network_mode: service:tailscale`; the sidecar has `TS_USERSPACE: "false"`, a kernel tun, `ip_forward=1`). Bridge traffic out of the dind therefore leaves through `tailscale0`: tailnet **IPs** should be reachable from a build step without extra flags.
- Tailnet **names** are the uncertain part. `entrypoint.sh` (lines 21-46) rewrites the dind's `/etc/resolv.conf` to `100.100.100.100` then `127.0.0.11`, because `network_mode: service:` shares the network namespace but not the mount namespace. Docker copies the daemon host's resolv.conf into containers on the default bridge (dropping loopback entries), so BuildKit and its `RUN` steps probably inherit `100.100.100.100`; nothing in the repo proves it.
- No image build reaches a tailnet service today. The builds' only external reach is Docker Hub, GitHub (`VM_PROVISION_GITHUB_TOKEN`) and the crates.io registry. The tailnet names that jobs on the VM already reach (`vm-ops-api-main`, `reporting-service`, `vnc-auth`) are reached by `wf` and by the services, from containers that sit in the sidecar's namespace themselves (`gh-runner`, `agent-sessions`, `agent-dind` all use `network_mode: service:tailscale`), never from a BuildKit container. The only earlier host-network precedent is `docs/solutions/build-errors/every-layer-that-needs-the-proxy-ca-separately.md` (`--driver-opt network=host`), written for a sandbox proxy.
- Verdict: **the proxy can work for image builds**, in this order of preference: (1) default bridge with MagicDNS if the probe below resolves the name; (2) `docker buildx create ... --driver-opt network=host`, for which the build's `RUN` steps also need `--network host --allow network.host`, but then the dind's own resolver and `tailscale0` apply directly; (3) a tailnet IP, which changes on recreate and is a last resort. `build_image.rs` line 120 is the one place that changes for (2).
- Other callers need the same name: the runner's CI jobs (`typecheck-lint-unit-tests`, clippy, `cargo-flow`: `gh-runner` is in the sidecar's namespace but has its own resolv.conf), sandbox agent sessions (egress is HTTPS-only; they reach the tailnet through the SOCKS5 listener, `WF_TAILNET_SOCKS`, `docs/solutions/infra/sandbox-joins-tailnet-instead-of-funnel.md`, and Cargo would need `http.proxy = "socks5h://localhost:1055"`), and a laptop on the tailnet. Each is an open point below.

### Evidence: which host

Recommended host: **a new tiny Rust service in its own workspace, `backend/libs-proxy`** (a `core`/`rs` crate pair like `reporting-service`, its own tailscale sidecar and hostname).

- **Not vm-ops-api.** It ships first and builds itself at the commit (`POST /redeploy?ref=`), and it is a consumer of `service-kit`, so a registry outage would block the build of the very service that deploys everything else and the proxy's own fix would need the proxy. Its auth model is a bearer token with routes that act on the VM; a public-ish GET proxy does not belong beside them.
- **Not reporting-service.** It stores samples, is a consumer of `service-kit` too, and its redeploy would interrupt library downloads of a concurrent build.
- **A new service** is the only host with no dependency on the registry it serves (the bootstrap rule: it depends on crates.io and nothing from `mla`, so it can always be built and shipped), a blast radius of one read path, and its own image hash (a change elsewhere never redeploys it). The cost is one more service leg and one more compose and tailscale sidecar.
- **Hexagonal shape** (`docs/solutions/architecture/hexagonal-architecture-by-package-type.md`, Rust services are `core`/`rs` pairs): `core` is pure (parse a request path into a `LibObject` key, reject `..`, empty and non-`libs/` shapes, map `dl/<crate>/<version>.crate` to `libs/crates/...`, decide the response from an `ObjectRead` result) and owns the port `ObjectRead::get(key, if_none_match) -> Found{bytes|stream, etag} | NotModified | Missing | Unavailable`; `rs` has the adapters (the OCI PAR reader over HTTPS, the HTTP listener) and `main.rs` as the one composition root; the in-memory double ships behind a `testing` entry. `pnpm run arch:services` must stay green.
- **Failure mode.** Proxy down means: a build whose image hash changed and that needs a crate it has not cached fails at `cargo chef cook`; everything that did not change builds nothing (the hash is unchanged), and a rebuild of a service whose pinned versions are already in the BuildKit cargo registry cache mount (`--mount=type=cache,target=/usr/local/cargo/registry` in each Dockerfile, kept up to 60 GB by `build_image.rs`) still succeeds offline when the layer is cached. Only a new pinned version or a cold cache fails, and the error is a plain connection refusal with the proxy's `/health`. Fallback for that case, to prove rather than assume: Cargo source replacement (`[source.mla] replace-with = "mla-direct"`) keeps the lockfile's recorded source while a second, secret-carrying config (`LIBS_READ_PAR_URL` as a BuildKit secret, never in a layer, never committed) points the same bytes at the bucket directly. Checksums in `Cargo.lock` make the two interchangeable.

### What `wf lib` (`clis/lib`) changes

1. `domain/index.rs`: `config.json` `dl` becomes the proxy template (`<proxy base>/dl/{crate}/{version}.crate`), not `<read PAR>crates/...`. The base is a public constant from `libraries.json` (`registries.mla.indexUrl`), so no token reaches the index.
2. `adapters/bucket_sparse.rs`: one `BlobStore` for crates and index, with the index under `libs/index/`; `read_base` stops being the PAR and becomes the proxy base. Write order stays (crate first, then index line, then `config.json`).
3. `LIBS_INDEX_WRITE_PAR_URL` and the second store go away; `LIBS_READ_PAR_URL` moves from "image builds" to the proxy's environment (`registry.json` description updated, `wf secret sync-env --for libs-proxy`). `testing.rs` (`INDEX_BUCKET`, `public_url`) collapses to one bucket plus a proxy double.
4. `usecases/retarget.rs` and `domain/lockfile.rs` need no change (a literal `indexUrl` already wins; `wf lib status` never prints a source URL). `wf lib status` gains a probe of the proxy `/health` and of `config.json` through it.
5. Terraform (`infra/oracle-cloud/build-cache.tf`): drop the public index bucket and its write PAR after migration; keep the `libs/` PAR (read for the proxy, write for `wf lib`) and keep `libs/` out of the lifecycle expiry.

### Steps

1. Run the live probes below; settle name vs IP and the builder network.
2. Build `backend/libs-proxy` (core, rs, Dockerfile, compose with a tailscale sidecar, `TS_HOSTNAME` fixed to a name nothing else uses: `vm-ops-api-main-1` exists as a stale duplicate in `clis/deploy/tests/flow/tailnet.rs`), register it as a deployable (`IMAGES` in `build_image.rs`, `wf-vmops` services and health URL, `local.vm_apps`, the ACL tag, a `LIBS_READ_PAR_URL` entry in the proxy's secret group). It ships before any consumer migrates.
3. Change `wf lib` as above, release the spike crates (`rca-core`, `rca-ladder`) to the scratch prefix and read them through the proxy.
4. Apply the builder-network change if the probe needs it, then migrate `remote-chat-agent` (plan order step 1) with a lock that records the proxy URL.
5. Remove the public index bucket once no `Cargo.lock` names it.

### Verification ladder (most important first)

1. **Runbook, live:** `curl` from inside the dind (`docker run --rm` on the default bridge) of `http://<proxy>/index/config.json`, then a `docker buildx build` of a one-line Dockerfile whose `RUN` does the same, on the real `agent-shell-builder`; record which of the three network options works. Confirm from outside the tailnet that the proxy, the old index bucket URL and the PAR give nothing (a negative probe must prove the request arrived: the proxy's own log line, or a non-DNS error).
2. **Deployment verification** (`test:verify:deployed` of `libs-proxy`): real `/health`, `config.json` and a real released `.crate` through the deployed proxy, bytes equal to the bucket object; `If-None-Match` answers 304; a `..`, a `libs/../x` and a non-GET answer 4xx.
3. **Integration:** the built `libs-proxy` binary as a black box against a fake PAR server (found, missing, upstream 5xx -> 502, ETag pass-through, large body streamed).
4. **Flow:** `wf lib release` through the in-memory double, then real `cargo` resolving and downloading through the proxy double over loopback (the existing flow test shape); asserts the committed `Cargo.lock` and `config.json` contain no PAR token.
5. **Unit:** `core` path mapping and rejection cases only where behavior is non-obvious.
6. **End to end:** the spike image build of `remote-chat-agent` through the proxy in CI (`wf ship ... --wait-serving`), cold and warm seconds against the plan's pass line, plus a build with the proxy stopped to show the warm-cache and fallback behavior above.

### Open points that need a live look at the VM

- Does a name like `libs-proxy.taila61461.ts.net` resolve inside a BuildKit `RUN` on the default bridge (the buildkit container's own `/etc/resolv.conf`), and does `100.100.100.100` answer from there? If not: `network=host` builder or the IP.
- Does the runner's CI jobs' resolv.conf include MagicDNS (gh-runner has the sidecar's network but its own mount namespace, same as the dind before `entrypoint.sh`)? How does `wf` resolve there today (`wf vm net resolve`)?
- Does a Docker Hub or crates.io fetch still work on a `network=host` builder inside the dind (DNS order: MagicDNS first, then 127.0.0.11, per `entrypoint.sh`)?
- Cargo through a SOCKS5 listener from sandbox sessions (`http.proxy = socks5h://...`), and from a laptop: is each expected to build the services, or only CI and the VM?
- Cargo source replacement keeping the lockfile's source while pointing at a secret-carrying direct registry: confirm with the installed cargo and `--locked`.
- The tailnet ACL: the proxy's tag must accept the runner and the dind (both `tag:mobile-learning`) and nothing else.
- If tailnet names cannot be made reachable from the builds, the best alternative is the same proxy exposed on the public Funnel with a Cargo token (`auth-required: true`, `CARGO_REGISTRIES_MLA_TOKEN` as a BuildKit `env=` secret, the token in the vault): it keeps the index URL non-secret and the crates private, at the price of a public listener and a token in each build.

## Migrated: every consumer takes the libraries from the registry

- `libraries.json` libraries (service-kit, telemetry, rca-core, rca-ladder) are released to `mla` (0.1.1 each). Every service workspace, the clis workspace, `clis/vnc-auth-capture/listener` depend on them as `{ version = "0.1", registry = "mla" }`; the registry is named in the root `.cargo/config.toml` (`sparse+http://libs-proxy:8793/index/`, no credential: the proxy is tailnet-only and holds the read link). A library's own manifest names a sibling by `path` beside `version` and `registry` (service-kit on telemetry, rca-ladder on rca-core); `wf check --budget` (`versioned-library-path-deps`, gated) refuses a path dependency anywhere else.
- A published crate holds only its own files: service-kit keeps a copy of the secrets registry (`backend/service-kit/data/registry.json`), a flow test keeps it equal to `clis/secrets/src/data/registry.json`; a registry change reaches the services with the next service-kit release. `clis/telemetry` is its own workspace (a clis member of the same name made `-p telemetry` ambiguous).
- Docker builds: no Dockerfile copies library sources (only `logger-cli`, which builds telemetry itself). `.cargo/config.toml` is copied into the `chef` stage; `cargo chef prepare/cook` and `cargo build --locked` fetch the pinned versions from the proxy. Probed live on the real `agent-shell-builder` (default bridge, no `network=host`): a `RUN` resolves `libs-proxy` through MagicDNS (`nameserver 100.100.100.100`, `search taila61461.ts.net`) and reads `/index/config.json`. No builder change was needed and the Funnel fallback is not used.
- Caching: the image hash already reads the Cargo.lock entries (name, version, checksum), so a service hash moves only when its lock pins a new release; the Nx cargo tasks key on the workspace `Cargo.toml` and `Cargo.lock` because the plugin derives dependency edges from `path` only. A library edit with no release redeploys nothing; adopting a release is one service's lock change.
- `wf lib status` shows `pins[].behind` per service (0 = on the latest); entries with `source: path` are the libraries' own workspaces.

### Local loop: edit a library against a service

Patch the registry crate for one cargo call, from the service's workspace, without touching a manifest: `cargo test --config 'patch.mla.service-kit.path="../service-kit"'` (also `cargo build`, `cargo run`). Cargo rewrites that workspace's Cargo.lock for the patch: do not commit it (`git checkout Cargo.lock`). To ship the change: `wf lib release <library> patch|minor|major` (dependency order: telemetry, rca-core, then service-kit, rca-ladder), then raise the version in the service's lock (`cargo update -p <library>`) and commit it. From a machine off the tailnet (the sandbox), reach `libs-proxy:8793` through the SOCKS listener (`wf vm net up`) with a local forward and a hosts entry for `libs-proxy`.

## Not in this plan

Renovate or automatic bumping (dropped earlier), publishing to crates.io now, changing the app's EAS Update flow, a registry server.
