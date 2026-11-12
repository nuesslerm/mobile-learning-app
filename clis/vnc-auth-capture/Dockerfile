# syntax=docker/dockerfile:1
#
# vnc-auth-capture auth-session image.
#
# It bundles two things:
#   1. A headless Chromium (CDP on loopback) and the tool CLIs whose login it completes. The base image is a linuxserver desktop,
#      pinned to a digest, kept for its s6 init and its `abc` user; its desktop services are removed below.
#   2. The persistent listener (`vnc-auth-listener`: the code of `wf secret login listener`, POST /auth/start) that
#      activates a login session and returns the tokened `tailscale serve` link. It is also the viewer: it serves JPEG frames
#      of the browser's page and relays taps and keys back over CDP.
#
# The only ingress is the auth-session container's own tailscale-serve sidecar
# (docker-compose.auth.yml); the browser's CDP port stays bound to loopback.

# Dependencies first: cargo-chef compiles what the manifests and the lockfile name into an image layer, which `mode=max`
# exports and a recreated VM imports; a source change then rebuilds the workspace crates alone.
FROM docker.io/library/rust:1-bookworm AS chef
RUN --mount=type=cache,id=cargo-registry-vnc-auth-capture,target=/usr/local/cargo/registry \
    cargo install cargo-chef --locked --version 0.1.78
WORKDIR /src
# The shared libraries (telemetry, rca-core) come from the mla registry (a public bucket, plain HTTPS); the lockfile pins their versions.
COPY .cargo/config.toml /src/.cargo/config.toml

# The listener alone (listener/), compiled with the crates of its cargo closure (wf-login and what it builds on), not the whole
# wf. The clis/ sources copied here are exactly the vnc-auth-session leg's trigger paths in clis/pipeline/src/legs.rs, which a
# flow test of deploy compares with this file and with the listener's manifests.
FROM chef AS planner
COPY clis/cdp ./clis/cdp
COPY clis/http ./clis/http
COPY clis/wf-kit ./clis/wf-kit
COPY clis/wf-login ./clis/wf-login
COPY clis/secrets ./clis/secrets
COPY clis/wf-testkit ./clis/wf-testkit
COPY clis/wf-vmops ./clis/wf-vmops
COPY clis/secrets/src/data/registry.json ./clis/secrets/src/data/registry.json
COPY clis/vnc-auth-capture/listener/Cargo.toml clis/vnc-auth-capture/listener/Cargo.lock ./clis/vnc-auth-capture/listener/
COPY clis/vnc-auth-capture/listener/src ./clis/vnc-auth-capture/listener/src
# Outside the workspace, so not in the recipe: a manifest and an empty file per source keep the dependency layer off their code.
RUN find /src/clis/cdp /src/clis/http /src/clis/wf-kit /src/clis/wf-login /src/clis/secrets /src/clis/wf-testkit /src/clis/wf-vmops \( -name Cargo.toml -o -name '*.rs' \) -exec sh -c 'for f; do mkdir -p "/skeleton${f%/*}"; case "$f" in *.rs) : > "/skeleton$f" ;; *) cp "$f" "/skeleton$f" ;; esac; done' sh {} +
WORKDIR /src/clis/vnc-auth-capture/listener
RUN --mount=type=cache,id=cargo-registry-vnc-auth-capture,target=/usr/local/cargo/registry \
    cargo chef prepare --recipe-path /recipe.json

FROM chef AS listener
WORKDIR /src/clis/vnc-auth-capture/listener
COPY --from=planner /recipe.json /recipe.json
COPY --from=planner /skeleton/src /src
RUN --mount=type=cache,id=cargo-registry-vnc-auth-capture,target=/usr/local/cargo/registry \
    cargo chef cook --recipe-path /recipe.json --release
COPY --from=planner /src /src
# Every source must be newer than the stub the cook layer compiled, or cargo keeps the stub.
RUN --mount=type=cache,id=cargo-registry-vnc-auth-capture,target=/usr/local/cargo/registry \
    find /src/clis -not -path '/src/clis/vnc-auth-capture/listener/target*' -name '*.rs' -exec touch {} + \
    && cargo build --release \
    && cp target/release/vnc-auth-listener /vnc-auth-listener

# PINNED to a digest, not the floating `ubuntu-xfce` tag. Unpinned, every
# rebuild could pull a different Chromium, so a desktop regression (window
# not fullscreen, a missing extension button) could never be attributed to a
# code change with any confidence -- see docs/known-gaps.md #58. The
# package's own security checklist in README.md already required this.
# This digest is what the tag resolved to on 2026-09-08; bump it
# deliberately, and re-run the desktop checks when you do.
FROM lscr.io/linuxserver/webtop:ubuntu-xfce@sha256:1bd141d5d7aaf3e98e47b7d9665f50657d1628617b4ef47bc3bbd43d726fd77e

# The `tailscale` CLI (client only, not tailscaled) -- this container shares
# the sidecar's NETWORK namespace (docker-compose.auth.yml's
# `network_mode: service:tailscale`), never its filesystem, so it has no
# `tailscale` binary of its own. The listener shells out to it
# (`vnc-auth-listener`) to self-configure its link base URL
# and bind its `tailscale serve` route -- confirmed live: without this
# binary present, both calls threw "command not found" and crash-looped the
# listener before it ever bound its HTTP port. Copied straight from the same
# image tag the sidecar runs, not reinstalled via apt/an installer script --
# no network call, and guaranteed version-matched.
COPY --from=tailscale/tailscale:v1.102.2 /usr/local/bin/tailscale /usr/local/bin/tailscale

# Node runtime, for installing the tool CLIs below (claude, codex: npm packages), from this base image's own apt sources (plain HTTP,
# ports.ubuntu.com) rather than curling nodejs.org directly: avoids needing
# any HTTPS/CA trust setup for this step at all, unlike a direct nodejs.org
# fetch (which a TLS-intercepting build-time proxy -- e.g. a
# corporate/sandboxed CI egress policy -- would otherwise block; nodejs.org
# and deb.nodesource.com are not universally on such an allowlist the way OS
# package mirrors typically are). `nodejs` only, not `nodejs npm`: on arm64
# this base image's resolvable `nodejs` candidate is itself nodesource-built
# (confirmed live from its resolved version string, `*-1nodesource1`, despite
# this file previously assuming a plain Ubuntu-universe package) and already
# bundles npm -- it formally Conflicts with the distro's separate `npm`
# package, so requesting both in the same apt-get install is unsatisfiable.
RUN apt-get update && apt-get install -y --no-install-recommends nodejs \
  && rm -rf /var/lib/apt/lists/*

# curl is what `scripts/setup.sh` downloads installers with; the listener itself shells out to neither curl nor openssl (it
# carries its own HTTP client, and the token gate speaks TLS to the desktop's self-signed port itself). Naming them keeps
# the image from depending on the base image shipping them.
RUN apt-get update && apt-get install -y --no-install-recommends curl ca-certificates \
  && rm -rf /var/lib/apt/lists/*

# Tool CLIs the descriptors log in (claude, codex). On-demand, best-effort --
# `|| true` alone only stops a failed npm-registry fetch from failing the
# BUILD; it silently ships an image missing claude/codex entirely, which is
# the entire point of this container.
#
# --mount=type=secret,id=extra_ca_cert,required=false: opt-in extra CA trust
# for a TLS-intercepting build-time network (a corporate/sandboxed CI egress
# proxy) -- inert by default. Verified live: pointing NODE_EXTRA_CA_CERTS at a
# path that doesn't exist (the normal case, when no --secret is passed) just
# prints a harmless "Ignoring extra certs ... no such file" warning and falls
# through to Node's normal trust store.
#
# Its only real input is this one script, so it is copied from the build
# CONTEXT and run BEFORE anything derived from the app. It used to run as
# /app/scripts/setup.sh, downstream of the whole compiled app, which made every
# source edit re-run two minutes of npm-registry traffic (measured 145s).
COPY clis/vnc-auth-capture/scripts/setup.sh /opt/vnc-auth/setup.sh
RUN --mount=type=secret,id=extra_ca_cert,required=false \
  NODE_EXTRA_CA_CERTS=/run/secrets/extra_ca_cert bash /opt/vnc-auth/setup.sh all || true

# The base image is a whole desktop: X server, XFCE, Selkies, pulseaudio, nginx. The login browser is a headed Chromium on a bare
# Xvfb screen and the listener is its own viewer, so none of those services starts: only the listener's and Xvfb's stay in the
# user bundle (added below, after this removes the base image's). Xvfb must exist: the build fails here when it does not.
RUN command -v Xvfb >/dev/null || (apt-get update && apt-get install -y --no-install-recommends xvfb && rm -rf /var/lib/apt/lists/*)
RUN cd /etc/s6-overlay/s6-rc.d/user/contents.d && for service in svc-*; do case "$service" in svc-vnc-auth|svc-xvfb) ;; *) rm -f "$service" ;; esac; done

# Init hooks (MagicDNS resolver, headless Chromium wrapper) and the listener as an s6 service.
COPY clis/vnc-auth-capture/docker/custom-cont-init.d/ /custom-cont-init.d/
COPY clis/vnc-auth-capture/docker/s6/ /etc/s6-overlay/s6-rc.d/
RUN chmod +x /custom-cont-init.d/* /etc/s6-overlay/s6-rc.d/svc-vnc-auth/run /etc/s6-overlay/s6-rc.d/svc-xvfb/run \
  && touch /etc/s6-overlay/s6-rc.d/user/contents.d/svc-vnc-auth /etc/s6-overlay/s6-rc.d/user/contents.d/svc-xvfb

# Last, so a listener change rebuilds and pushes only this layer, never the Node, npm and desktop layers above.
COPY --from=listener /vnc-auth-listener /usr/local/bin/vnc-auth-listener

ENV VNC_AUTH_LISTENER_PORT=8795 \
    DISPLAY=:99
