# syntax=docker/dockerfile:1
#
# Built from the REPO ROOT context (not this dir): the Rust workspace manifest lives in backend/. Built natively for its architecture
# by the deploy leg of CI (`wf ship build leetcode-backend --tag <tag>`).
#
# The service is one static-ish Rust binary (backend/leetcode-backend/rs). The runtime image carries what the executor
# spawns on demand: Python (the language being run, plus `black` for formatting) and Node (for JavaScript submissions).
# Neither runs unless a /run or /format request asks for it, so the idle service is the binary alone.

# Source of the CA bundle used to switch apt to HTTPS (see the runtime stage).
FROM alpine:3.20 AS aptcerts
RUN apk add --no-cache ca-certificates

# Dependencies first: cargo-chef compiles what the manifests and the lockfile name into an image layer, which `mode=max`
# exports and a recreated VM imports; a source change then rebuilds the workspace crates alone.
FROM docker.io/library/rust:1-bookworm AS chef
RUN --mount=type=cache,id=cargo-registry-leetcode-backend,target=/usr/local/cargo/registry \
    cargo install cargo-chef --locked --version 0.1.78
WORKDIR /src
# The shared libraries come from the mla registry (libs-proxy on the tailnet); the lockfile pins their versions.
COPY .cargo/config.toml /src/.cargo/config.toml

FROM chef AS planner
# Cargo loads every workspace member's manifest (and its path dependencies) even for `-p leetcode-backend-rs`, so the sibling
# service and its path crate must be present; neither is compiled here.
COPY backend/leetcode-backend/Cargo.toml backend/leetcode-backend/Cargo.lock ./backend/leetcode-backend/
COPY backend/leetcode-backend/core ./backend/leetcode-backend/core
COPY backend/leetcode-backend/rs ./backend/leetcode-backend/rs
WORKDIR /src/backend/leetcode-backend
# --bin: the recipe keeps this service's crates only, so a sibling's manifest does not move it.
RUN --mount=type=cache,id=cargo-registry-leetcode-backend,target=/usr/local/cargo/registry \
    cargo chef prepare --recipe-path /recipe.json --bin leetcode-backend-rs

FROM chef AS builder
WORKDIR /src/backend/leetcode-backend
COPY --from=planner /recipe.json /recipe.json
# No --locked: the recipe's lockfile is cut to this service's packages; the build below checks the real one.
RUN --mount=type=cache,id=cargo-registry-leetcode-backend,target=/usr/local/cargo/registry \
    cargo chef cook --recipe-path /recipe.json --profile deploy -p leetcode-backend-rs
COPY --from=planner /src /src
# Every source must be newer than the stub the cook layer compiled, or cargo keeps the stub.
RUN --mount=type=cache,id=cargo-registry-leetcode-backend,target=/usr/local/cargo/registry \
    find /src/backend -not -path '/src/backend/leetcode-backend/target*' -name '*.rs' -exec touch {} + \
    && cargo build --profile deploy --locked -p leetcode-backend-rs \
    && cp target/deploy/leetcode-backend-rs /leetcode-backend-rs

FROM debian:bookworm-slim AS runtime
# ── apt over HTTPS ───────────────────────────────────────────────────────────
# Debian's default sources are plain `http://`. That cannot work behind a proxy which only permits HTTPS CONNECT — this
# repo's agent sandbox answers 405 to every non-CONNECT request, so `apt-get update` fails and no image here could be built
# from a session. Switching the sources to https:// needs a CA bundle, and the slim base ships none, so it is copied from
# an alpine stage. Harmless everywhere else: HTTPS apt is a plain improvement in CI too.
COPY --from=aptcerts /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
RUN sed -i 's|http://deb.debian.org|https://deb.debian.org|g; s|http://security.debian.org|https://security.debian.org|g' \
      /etc/apt/sources.list.d/debian.sources 2>/dev/null || true; \
    sed -i 's|http://deb.debian.org|https://deb.debian.org|g; s|http://security.debian.org|https://security.debian.org|g' \
      /etc/apt/sources.list 2>/dev/null || true

# Optional: trust a TLS-terminating egress proxy's CA for the rest of the build. Agent sandboxes route egress through a MITM
# proxy that presents its own CA, so pip and curl would fail certificate verification. Passed as a BuildKit secret, never
# committed (`docker buildx build --secret id=proxy_ca,src=/root/.ccr/ca-bundle.crt ...`); a no-op when absent, which is the
# CI and release case, so no published image ever trusts an extra CA.
RUN --mount=type=secret,id=proxy_ca \
    if [ -s /run/secrets/proxy_ca ]; then \
      apt-get update && apt-get install -y --no-install-recommends ca-certificates && \
      cp /run/secrets/proxy_ca /usr/local/share/ca-certificates/egress-proxy.crt && \
      update-ca-certificates && \
      rm -rf /var/lib/apt/lists/*; \
    fi

# `python3` runs and formats Python submissions; `black` is the preferred formatter and the harness falls back to a stdlib `ast.unparse`
# pass when the import fails. --break-system-packages because Debian marks the system Python externally managed (PEP 668)
# and this is a single-purpose container with no other Python consumer. `nodejs` runs JavaScript submissions.
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates python3 python3-pip nodejs && \
    pip3 install --no-cache-dir --break-system-packages black && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /leetcode-backend-rs /usr/local/bin/leetcode-backend-rs
# This container runs `network_mode: service:tailscale` in docker-compose.prod.yml -- shares the sidecar's NETWORK namespace
# but not its MOUNT namespace, so Docker still writes its own /etc/resolv.conf pointing only at the embedded 127.0.0.11
# resolver, which has never heard of Tailscale MagicDNS. See docker-entrypoint.sh's own comment + docs/known-gaps.md.
COPY backend/leetcode-backend/docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh && mkdir -p /app/.data
WORKDIR /app
# Declared last: an ENV that changes every commit would otherwise invalidate every layer after it.
ARG GIT_COMMIT=unknown
ENV GIT_COMMIT=$GIT_COMMIT
EXPOSE 8789
ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
CMD ["leetcode-backend-rs"]
