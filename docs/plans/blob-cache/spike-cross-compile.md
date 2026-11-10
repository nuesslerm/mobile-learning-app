# Cache setup as built, and the smallest spike: cross-compiled binary in blob storage vs a VM Docker build

Status: 23 Oct 2026. The setup table is what `bf24a5999` to `b1fcb68bc` put on `dev`; the spike is proposed, nothing of it is built.

## The cache setup now

| What | Where it lives | Shared by sandbox and VM | Expires |
| --- | --- | --- | --- |
| Nx task results: lint, typecheck, unit and flow of the TypeScript packages, and the Rust `cargo-flow` passes | OCI bucket `mobile-learning-build-cache`, `nx/g<generation>/<hash>`, through a loopback shim in `wf` | yes: Node is pinned to 22.22.0, `rustc` to 1.97.0, and the platform is no longer part of a Rust task's hash, so an x86 pass replays on arm. The browser e2e task still hashes the platform (Chromium build). | 14 days |
| pnpm store | same bucket, `pnpm/g<generation>/store-v11.tar.gz` (`wf cache restore\|save pnpm`) | yes (arm-only native packages are fetched on the VM) | 60 days |
| Docker layers, including the cargo-chef dependency layer of the Rust images | Docker Hub, `docker.io/nuesslerm/<image>:buildcache`, `mode=max` | no: built on the VM (arm64) only; a sandbox layer is another platform | none (Hub) |
| Compiled Rust output | nowhere shared: the VM compiles natively inside the Docker build; the sandbox cannot warm it | no | |
| Disposable local state: `.nx/cache`, `target/`, BuildKit's local store | the machine | no | lost on a recreate, by design |

Gone: the Nx listener, the `nx-cache` volume and `wf vm ops cache*` in vm-ops-api. Nothing durable lives on the VM.
A recreated VM therefore finds its Nx results, its pnpm store and its Docker layers, and compiles only what changed.

## The question

Is it better for the Rust services to be compiled once for aarch64 in the sandbox (cross-compiled), uploaded to the blob bucket by source hash, and downloaded by the VM (whose image build then only copies the binary), or compiled on the VM by `docker buildx build` with the dependency layer cached on Docker Hub?

## The spike, as small as it can be

One service, one commit, two arms, measured from the start of the pipeline to the end of its deployment verification test.

- **Service:** `backend/reporting-service` (small, has `test:verify:deployed`). Nothing else is touched; no CI file changes.
- **Arm A, VM Docker build (today's setup).** `docker buildx build --platform linux/arm64` of the service's Dockerfile inside agent-shell on the VM, as `wf ship reporting-service --ref <sha> --wait-serving` does it. Cold: the build runs with `--no-cache` and without `--cache-from`, which is what a recreated VM with an empty builder and a cold registry would do. Warm: the same command after a source-only edit (the cook layer from the cold run is reused).
- **Arm B, cross-compile and blob.** In the sandbox: `rustup target add aarch64-unknown-linux-gnu` plus a linker (`aarch64-linux-gnu-gcc` from apt, or `cargo-zigbuild` with zig from pip), `cargo build --profile deploy --target aarch64-unknown-linux-gnu -p reporting-service`; the binary is gzip-ed and `PUT` through the write PAR to `probe/spike/<sha>/reporting-service.gz` (the 1-day lifecycle prefix, so nothing lingers). On the VM: download it through the read PAR into the build context and run the same Dockerfile reduced to a runtime stage (`COPY` of the binary) as a throw-away file under `.agents/tmp/`, then redeploy and wait. Cold (empty `target/`) and warm (source-only edit, the sandbox `target/` kept).
- **Measured, per arm and per temperature, n = 1:**
  - time: wall clock of each step and of the whole run, from the first step to the first green `test:verify:deployed` of the service (the test is run with `wf` against the VM, the same command CI runs);
  - CPU and peak memory: sandbox steps with `/usr/bin/time -v` (user and system seconds, max RSS); VM steps by reading agent-shell's cgroup before and after (`cpu.stat` `usage_usec`, `memory.peak`) around each job, because a `docker buildx build` runs in a child container;
  - space: the binary and its gzip, the blob bytes written, the sandbox `target/` size, the VM image size, and the Docker Hub `:buildcache` tag size before and after;
  - requests: the blob requests the arm made (counted by the shim's own counters or by hand).
- **Decision rule, written before the numbers:** adopt the binary route only if it saves at least 40 % of the cold end-to-end time on the VM leg and the sandbox peak RSS stays under 6 GB; otherwise keep the Docker build. Complexity (a cross toolchain in the sandbox, an arm linker, a second build path to keep green) is the cost the saving has to beat. Log the numbers and the decision in `docs/audits/2026-09-02-token-diet-decision-log.md`.
- **What would make it larger, and is out of the spike:** all services, a CI job that does the cross-compile, the Docker Hub cache for the runtime stage, `wf` as a prebuilt binary.

## Preconditions

1. The VM holds the new GitHub token (the VM is recreated from the vault value; the old one on the VM answers 401).
2. The two cache PAR secrets are in the vault (done) and the cache bucket exists (done, Terraform converges).
3. About 3 GB of sandbox disk for the cross `target/` of one service (the disk cleaner prunes stale duplicates first).
4. The pipeline is green on `dev` at the commit measured, so a timing is not a failure.
