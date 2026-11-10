---
title: "Docker CLI's own ~/.docker/config.json proxy silently overrides an explicit -e HTTPS_PROXY on docker run/exec"
date: 2026-08-17
category: build-errors
module: docker-build
problem_type: build_error
component: tooling
severity: medium
symptoms:
  - "A container gets ECONNREFUSED against 172.17.0.1:<port> (the Docker bridge gateway) even though -e HTTPS_PROXY=<explicit-value> was passed on the docker run/exec command line"
  - "npm/Node's own fetch stack inside the container appears to ignore an explicit uppercase HTTPS_PROXY/HTTP_PROXY passed via -e"
  - "The same command works fine when run directly on the host, but fails only inside a container"
root_cause: config_error
resolution_type: config_change
related_components:
  - clis/vnc-auth-capture
tags: [docker, proxy, docker-cli, econnrefused, env-vars]
---

# Docker CLI's own ~/.docker/config.json proxy silently overrides an explicit -e HTTPS_PROXY on docker run/exec

## Problem

A `docker run`/`docker exec` invocation with an explicit
`-e HTTPS_PROXY=<value>` still failed with `ECONNREFUSED` against
`172.17.0.1:<port>` — the Docker bridge network's own gateway address, not
the intended proxy — as if the explicit override had been ignored entirely.

## Symptoms

```
ECONNREFUSED 172.17.0.1:<port>
```

surfacing from an HTTP client (npm/Node's fetch stack) inside a container
launched with `-e HTTPS_PROXY=<explicit value>` on the command line, where
`<port>` did not match the intended proxy's port at all.

## What Didn't Work

- **Assuming the explicit `-e HTTPS_PROXY=...` flag was authoritative.**
  Docker CLI's own client config, `~/.docker/config.json`, carries a
  `proxies.default.httpsProxy`/`httpProxy` block that Docker automatically
  injects into every `docker run`/`docker exec` as **lowercase**
  `https_proxy`/`http_proxy` environment variables — independent of, and
  layered on top of, whatever `-e` flags are passed. npm/Node's HTTP client
  stack (and many other HTTP libraries) prefers the lowercase variant when
  both cases are present, so Docker's own auto-injected lowercase value won
  out over the explicit uppercase `-e HTTPS_PROXY` override, even though the
  override was genuinely present in the container's environment too.

## Solution

Pass the proxy override in **both** cases explicitly, rather than relying on
the uppercase form alone:

```bash
docker run -e HTTPS_PROXY="$PROXY_URL" -e HTTP_PROXY="$PROXY_URL" \
           -e https_proxy="$PROXY_URL" -e http_proxy="$PROXY_URL" \
           ...
```

With the lowercase variant explicitly set to the intended value, it no
longer matters that Docker's own config also injects a lowercase value —
the explicit `-e` flag on the command line takes precedence over whatever
`~/.docker/config.json` would otherwise contribute.

## Why This Works

Docker's client-side `proxies` config in `~/.docker/config.json` exists so
containers automatically pick up a user's proxy settings without needing
`-e` flags on every invocation — a convenience feature, but one that writes
lowercase environment variables into the container regardless of what else
is passed. Most HTTP client stacks (Node's fetch/undici, and many others
following the historical curl convention) check the lowercase
`https_proxy`/`http_proxy` variables preferentially, or in addition to the
uppercase ones, so an uppercase-only override doesn't fully suppress
Docker's own injected value — both variables end up present, and the
lowercase one (Docker's default, pointing at the bridge gateway) is the one
the HTTP client actually reads.

## Prevention

- **When overriding a container's proxy via `-e`, always set both the
  uppercase and lowercase variable names.** An uppercase-only override is
  not sufficient to suppress Docker CLI's own default proxy injection.
- **When debugging an unexpected `ECONNREFUSED` against `172.17.0.1` (or any
  Docker bridge-network gateway address) from inside a container**, check
  `~/.docker/config.json`'s `proxies.default` block before assuming the
  application or an explicit `-e` flag is at fault — that address is a
  strong signal Docker's own default proxy config is in play, not the
  proxy value passed on the command line. Do not paste the contents of
  `~/.docker/config.json` into logs, chat, or documentation without
  redacting it first — it also carries registry auth credentials (base64
  basic-auth tokens) alongside the proxy config, on the same file.

## Related Issues

- `docs/solutions/build-errors/every-layer-that-needs-the-proxy-ca-separately.md`
  — a different, adjacent set of proxy-related Docker build failures in this
  same sandbox (CA trust across build stages and the buildkit container's
  own network namespace during `docker buildx build`). That doc's failures
  are at build time, inside BuildKit; this doc's failure is at
  `docker run`/`docker exec` time, and the mechanism (Docker CLI's own
  default-proxy config injection) is distinct from CA trust or BuildKit's
  network namespace.
