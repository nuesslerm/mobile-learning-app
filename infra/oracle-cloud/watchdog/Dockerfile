# syntax=docker/dockerfile:1
#
# Built from the REPO ROOT context (the Rust workspace manifest lives in backend/). The daemon is one Rust binary
# (backend/watchdog/rs), built for musl so the runtime can be the alpine image that carries the docker CLI and the compose
# plugin it shells out to (`docker ps`, `docker inspect`, `docker compose up --force-recreate`).

# Dependencies first: cargo-chef compiles what the manifests and the lockfile name into an image layer, which `mode=max`
# exports and a recreated VM imports; a source change then rebuilds the workspace crates alone.
FROM docker.io/library/rust:1-alpine AS chef
RUN apk add --no-cache build-base
RUN --mount=type=cache,id=cargo-registry-watchdog,target=/usr/local/cargo/registry \
    cargo install cargo-chef --locked --version 0.1.78
WORKDIR /src
# The shared libraries come from the mla registry (a public bucket, plain HTTPS); the lockfile pins their versions.
COPY .cargo/config.toml /src/.cargo/config.toml

FROM chef AS planner
# This daemon is its own Cargo workspace (backend/watchdog/Cargo.toml and Cargo.lock): no sibling service is copied. The manifest and lockfile are copied as files so the image hash reads them.
COPY backend/watchdog/Cargo.toml backend/watchdog/Cargo.lock ./backend/watchdog/
COPY backend/watchdog/core ./backend/watchdog/core
COPY backend/watchdog/rs ./backend/watchdog/rs
WORKDIR /src/backend/watchdog
# --bin: the recipe keeps this service's crates only, so a sibling's manifest does not move it.
RUN --mount=type=cache,id=cargo-registry-watchdog,target=/usr/local/cargo/registry \
    cargo chef prepare --recipe-path /recipe.json --bin watchdog-rs

FROM chef AS builder
ENV CARGO_TARGET_DIR=/src/backend/watchdog/target-musl
WORKDIR /src/backend/watchdog
COPY --from=planner /recipe.json /recipe.json
# No --locked: the recipe's lockfile is cut to this service's packages; the build below checks the real one.
RUN --mount=type=cache,id=cargo-registry-watchdog,target=/usr/local/cargo/registry \
    cargo chef cook --recipe-path /recipe.json --profile deploy -p watchdog-rs
COPY --from=planner /src /src
# Every source must be newer than the stub the cook layer compiled, or cargo keeps the stub: libraries/ (rca-core, rca-ladder) too.
RUN --mount=type=cache,id=cargo-registry-watchdog,target=/usr/local/cargo/registry \
    find /src/backend -not -path '/src/backend/watchdog/target*' -name '*.rs' -exec touch {} + \
    && cargo build --profile deploy --locked -p watchdog-rs \
    && cp target-musl/deploy/watchdog-rs /watchdog-rs

FROM alpine:3.20 AS runtime
RUN apk add --no-cache docker-cli docker-cli-compose ca-certificates
COPY --from=builder /watchdog-rs /usr/local/bin/watchdog-rs
CMD ["watchdog-rs"]
