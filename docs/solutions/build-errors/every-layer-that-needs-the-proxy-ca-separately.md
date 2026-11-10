---
module: docker-build
date: 2026-08-13
problem_type: build_error
component: tooling
severity: high
symptoms:
  - "curl exits 60 fetching a release tarball, several minutes into an otherwise healthy build"
  - "npm install fails with SELF_SIGNED_CERT_IN_CHAIN even though the OS CA store was updated"
  - "ERR_PNPM_ENOSPC while downloading tarballs, with plenty of disk apparently free"
  - "x509: certificate signed by unknown authority resolving docker/dockerfile:1, before any stage runs"
  - "Could not connect to 127.0.0.1:<port> (111: Connection refused) from inside a build step"
root_cause: config_error
resolution_type: environment_setup
related_components:
  - remote-chat-agent
  - opencode-telegram-bot
tags:
  - docker
  - buildkit
  - egress-proxy
  - ca-certificates
  - sandbox
  - buildx
---

# Every layer needs the proxy CA separately, and each one fails differently

## Problem

Building a backend image from an agent sandbox behind a TLS-intercepting egress
proxy failed five times in a row. Every failure was the same underlying problem —
something in the chain did not trust or could not reach the proxy — and every
error message named something else entirely, several minutes downstream of the
layer at fault.

## Symptoms

In the order they were hit:

| Failure | Where it actually was |
|---|---|
| `curl … exit code 60` fetching the gh CLI tarball | the base stage never got the CA |
| `SELF_SIGNED_CERT_IN_CHAIN` from ffmpeg-static's installer | Node's own CA list, cleared by an empty env assignment |
| `ERR_PNPM_ENOSPC` mid-download | build cache grown to 21GB by the *previous* failures |
| `x509: certificate signed by unknown authority` resolving `docker/dockerfile:1` | the buildkit container's CA store |
| `Could not connect to 127.0.0.1:<port> (111: Connection refused)` | the buildkit container's network namespace |

None of these mention a proxy or a certificate authority in a way that points at
the layer to fix.

## What Didn't Work

- **Assuming the first `FROM` is the base of everything.** The build script
  inserted its CA-trust step after the first `FROM` on the reasoning that all
  later stages derive from it. True when written; false once the Dockerfile
  gained a leading helper stage nothing inherits from. The trust landed on a
  dead end and the real base image kept none of it.
- **Trusting a passing layer.** The step that installs the CA reported
  `DONE 0.1s` — because its `if [ -s /run/secrets/proxy_ca ]` guard was false and
  it did nothing at all. A fast success and a skipped no-op look identical.
- **Reading disk usage.** `df` showed ample space; the writable allowance was
  exhausted. `docker system df` was the view that mattered.

## Solution

**1. Patch every stage built from an external image, not just the first.** Track
declared stage names and insert the CA-trust step after each `FROM` that names
something outside the file:

```awk
/^FROM / {
  print
  image = ""
  for (i = 2; i <= NF; i++) { if ($i ~ /^--/) continue; image = $i; break }
  for (i = 2; i < NF; i++) if (tolower($i) == "as") stages[$(i + 1)] = 1
  if (!(image in stages)) trust_ca()
  next
}
```

**2. Never let a conditional env assignment clear an inherited value.** A
command-level `VAR="$(… || true)" cmd` sets `VAR` to the **empty string** when the
substitution is empty, overriding whatever the image already carried:

```dockerfile
# before — clobbers the ENV the build script installed
RUN --mount=type=secret,id=proxy_ca \
    NODE_EXTRA_CA_CERTS="$([ -s /run/secrets/proxy_ca ] && echo /run/secrets/proxy_ca || true)" \
    pnpm --filter=… deploy /app

# after — export only when there is something to export
RUN --mount=type=secret,id=proxy_ca \
    if [ -s /run/secrets/proxy_ca ]; then export NODE_EXTRA_CA_CERTS=/run/secrets/proxy_ca; fi; \
    pnpm --filter=… deploy /app
```

**3. Cap the build cache on every exit, not on success.** The cap ran as a plain
command after `docker buildx build`, which `set -e` skips entirely when the build
fails — so a run of failures grew the cache unboundedly. Moved into the `EXIT`
trap, preserving the original status:

```bash
cleanup() {
  local status=$?
  [[ -n "$CLEANUP_CA" ]] && rm -f $CLEANUP_CA
  "$REPO_ROOT/scripts/docker-cache-cleanup.sh" || true
  return $status
}
trap cleanup EXIT
```

**4. Trust the CA inside the buildkit container too.** With the
`docker-container` driver, buildkit runs in its own container with its own CA
store, and it — not the daemon, not any build stage — resolves the `# syntax=`
directive and every base-image manifest. The build script now appends the CA to
the active builder's store and restarts buildkitd (Go caches system roots on
first use), with a marker file making re-runs a no-op.

**5. Give the buildkit container host networking.** The egress proxy listens on
**host** loopback. With the `docker-container` driver, `--network host` on a
*build* means the buildkit container's namespace — not the host's — so a builder
on the default bridge cannot reach the proxy at all. The script provisions a
builder with `--driver-opt network=host` rather than trusting whichever builder
happens to be active.

## Why This Works

The request path has four independent trust/reachability boundaries — the build
stage's OS store, the build stage's Node CA list, the buildkit container's OS
store, and the buildkit container's network namespace. Each is configured
separately, and a gap in any one produces a failure attributed to whatever
happened to be downloading at the time.

The cache cap belongs in the `EXIT` trap because a *failed* build is when it
matters most: partial layers are still written, so the runs that need the cap are
exactly the runs that skipped it.

## Prevention

**A "the first stage is the base" assumption has an expiry date.** It is true
until someone adds a stage above it. Derive the property (which `FROM`s name
external images) instead of encoding a position.

**A guarded step that does nothing reports success.** `DONE 0.1s` on a step meant
to install a CA means the guard was false. When a conditional step is load-bearing,
have it say which branch it took, or verify the artifact afterwards.

**`VAR="$(cmd || true)" prog` is not "leave it alone".** It is "set it to
empty" — which beats an inherited `ENV`. Use `if … then export … fi` when the
intent is conditional.

**Cleanup belongs in a trap.** Any `set -e` script whose cleanup follows the
fallible command skips that cleanup on exactly the runs that need it.

**With the `docker-container` driver, the builder is a separate host.** It has its
own certificate store and its own network namespace, and `--network host` on a
build refers to *its* namespace. Anything a build needs to reach on host loopback
requires the builder itself to be on the host network.

**Provision the builder rather than inheriting it.** The ambient buildx builder
is wrong by default in this environment, and the failure it produces
(`connection refused` to a loopback port) points nowhere near the cause. The
build script now creates and configures the builder it needs.
