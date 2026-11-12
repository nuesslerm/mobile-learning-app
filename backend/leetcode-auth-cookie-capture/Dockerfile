# syntax=docker/dockerfile:1
#
# Built from the REPO ROOT context (see the deploy workflow). The service is one Rust binary (backend/leetcode-auth-cookie-capture/rs);
# the runtime keeps the Playwright base image because its Chromium and OS libraries are what `wf browser` drives here. Node is in that
# image but nothing in it runs Node. Only ever run in CAPTURE_MODE=remote (headed Chromium on Xvfb, reached through noVNC).

# Source of the CA bundle used to switch apt to HTTPS (see the runtime stage).
FROM alpine:3.20 AS aptcerts
RUN apk add --no-cache ca-certificates

# Dependencies first: cargo-chef compiles what the manifests and the lockfile name into an image layer, which `mode=max`
# exports and a recreated VM imports; a source change then rebuilds the workspace crates alone.
FROM docker.io/library/rust:1-bookworm AS chef
RUN --mount=type=cache,id=cargo-registry-leetcode-auth-cookie-capture,target=/usr/local/cargo/registry \
    cargo install cargo-chef --locked --version 0.1.78
WORKDIR /src
# The shared libraries come from the mla registry (a public bucket, plain HTTPS); the lockfile pins their versions.
COPY .cargo/config.toml /src/.cargo/config.toml

FROM chef AS planner
# This service is its own Cargo workspace (backend/leetcode-auth-cookie-capture/Cargo.toml and Cargo.lock): no sibling service is copied, and a dependency change
# in another service cannot move this image. The manifest and lockfile are copied as files so the image hash reads them (clis/pipeline/src/image_hash.rs).
COPY backend/leetcode-auth-cookie-capture/Cargo.toml backend/leetcode-auth-cookie-capture/Cargo.lock ./backend/leetcode-auth-cookie-capture/
COPY backend/leetcode-auth-cookie-capture/core ./backend/leetcode-auth-cookie-capture/core
COPY backend/leetcode-auth-cookie-capture/rs ./backend/leetcode-auth-cookie-capture/rs
WORKDIR /src/backend/leetcode-auth-cookie-capture
# --bin: the recipe keeps this service's crates only, so a sibling's manifest does not move it.
RUN --mount=type=cache,id=cargo-registry-leetcode-auth-cookie-capture,target=/usr/local/cargo/registry \
    cargo chef prepare --recipe-path /recipe.json --bin leetcode-auth-cookie-capture-rs

FROM chef AS builder
WORKDIR /src/backend/leetcode-auth-cookie-capture
COPY --from=planner /recipe.json /recipe.json
# No --locked: the recipe's lockfile is cut to this service's packages; the build below checks the real one.
RUN --mount=type=cache,id=cargo-registry-leetcode-auth-cookie-capture,target=/usr/local/cargo/registry \
    cargo chef cook --recipe-path /recipe.json --profile deploy -p leetcode-auth-cookie-capture-rs
COPY --from=planner /src /src
# Every source must be newer than the stub the cook layer compiled, or cargo keeps the stub.
RUN --mount=type=cache,id=cargo-registry-leetcode-auth-cookie-capture,target=/usr/local/cargo/registry \
    find /src/backend -not -path '/src/backend/leetcode-auth-cookie-capture/target*' -name '*.rs' -exec touch {} + \
    && cargo build --profile deploy --locked -p leetcode-auth-cookie-capture-rs \
    && cp target/deploy/leetcode-auth-cookie-capture-rs /leetcode-auth-cookie-capture-rs

FROM mcr.microsoft.com/playwright:v1.49.1-jammy AS runtime
# apt over HTTPS: Debian's default sources are plain http://, which cannot work behind a proxy that only permits HTTPS CONNECT (this
# repo's agent sandbox answers 405 to every other method). The CA bundle comes from the alpine stage, no network needed.
COPY --from=aptcerts /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
RUN sed -i 's|http://deb.debian.org|https://deb.debian.org|g; s|http://security.debian.org|https://security.debian.org|g' \
      /etc/apt/sources.list.d/debian.sources 2>/dev/null || true; \
    sed -i 's|http://archive.ubuntu.com|https://archive.ubuntu.com|g; s|http://security.ubuntu.com|https://security.ubuntu.com|g' \
      /etc/apt/sources.list 2>/dev/null || true
# Optional: trust a TLS-terminating egress proxy's CA for the rest of the build (a BuildKit secret, never committed; a no-op when
# absent, which is the CI and release case).
RUN --mount=type=secret,id=proxy_ca \
    if [ -s /run/secrets/proxy_ca ]; then \
      apt-get update && apt-get install -y --no-install-recommends ca-certificates && \
      cp /run/secrets/proxy_ca /usr/local/share/ca-certificates/egress-proxy.crt && \
      update-ca-certificates && \
      rm -rf /var/lib/apt/lists/*; \
    fi
# DEBIAN_FRONTEND=noninteractive is REQUIRED: xvfb pulls in tzdata, whose postinst asks an interactive question; there is no TTY in a
# build, so without it apt-get hangs instead of failing.
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt/lists,sharing=locked \
    DEBIAN_FRONTEND=noninteractive apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      xvfb x11vnc novnc websockify
WORKDIR /app
# wf: the VM's one build on the `clis` volume (clis.yml), mounted at /opt/clis.
RUN ln -sf /opt/clis/current/wf /usr/local/bin/wf
COPY --from=builder /leetcode-auth-cookie-capture-rs /usr/local/bin/leetcode-auth-cookie-capture-rs
# This container runs `network_mode: service:tailscale` in docker-compose.capture.yml: it shares the sidecar's network namespace but not
# its mount namespace, so the entrypoint adds Tailscale's MagicDNS resolver to /etc/resolv.conf (LEETCODE_BACKEND_BASE_URL is a *.ts.net
# hostname).
COPY backend/leetcode-auth-cookie-capture/docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh
ARG GIT_COMMIT=unknown
ENV GIT_COMMIT=$GIT_COMMIT
EXPOSE 6080
ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
# docker-compose.capture.yml runs the persistent listener; a bare run takes one attempt with `--once`.
CMD ["leetcode-auth-cookie-capture-rs"]
