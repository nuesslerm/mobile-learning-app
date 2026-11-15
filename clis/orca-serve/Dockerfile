# syntax=docker/dockerfile:1
#
# `orca-serve serve`: the Orca-compatible host in front of rca's agent API, one Rust binary built from the repo root context. Only the cargo
# closure of the binary is copied (wf-kit, the orca-* crates, the agent-api crates it speaks to rca with), and the clis workspace is trimmed to
# those members so the other clis crates cannot move this image. The runtime holds the binary and a CA bundle.
FROM alpine:3.20 AS aptcerts
RUN apk add --no-cache ca-certificates

# Dependencies first: cargo-chef compiles what the manifests and the lockfile name into an image layer, which `mode=max` exports and a recreated VM
# imports; a source change then rebuilds the workspace crates alone.
FROM docker.io/library/rust:1-bookworm AS chef
RUN --mount=type=cache,id=cargo-registry-orca-serve,target=/usr/local/cargo/registry \
    cargo install cargo-chef --locked --version 0.1.78
WORKDIR /src
# The shared libraries come from the mla registry (a public bucket, plain HTTPS); the lockfile pins their versions.
COPY .cargo/config.toml /src/.cargo/config.toml

FROM chef AS planner
COPY clis/Cargo.toml clis/Cargo.lock ./clis/
COPY clis/wf-kit ./clis/wf-kit
COPY clis/orca-serve ./clis/orca-serve
# The wire contracts are copied one crate at a time: `wf` hashes the image from the crates its Dockerfile names, and a whole-directory COPY
# would be dropped from the hash because that directory holds a workspace manifest. Their tests are not in the context (.dockerignore).
COPY libraries/agent-api/agent-api-accounts ./libraries/agent-api/agent-api-accounts
COPY libraries/agent-api/agent-api-agent ./libraries/agent-api/agent-api-agent
COPY libraries/agent-api/agent-api-attachments ./libraries/agent-api/agent-api-attachments
COPY libraries/agent-api/agent-api-catalog ./libraries/agent-api/agent-api-catalog
COPY libraries/agent-api/agent-api-terminal ./libraries/agent-api/agent-api-terminal
COPY libraries/agent-api/agent-api-client ./libraries/agent-api/agent-api-client
COPY libraries/agent-api/agent-api-git ./libraries/agent-api/agent-api-git
COPY libraries/agent-api/agent-api-hosted ./libraries/agent-api/agent-api-hosted
COPY libraries/agent-api/agent-api-schedules ./libraries/agent-api/agent-api-schedules
COPY libraries/agent-api/agent-api-proto ./libraries/agent-api/agent-api-proto
COPY libraries/agent-api/agent-api-service ./libraries/agent-api/agent-api-service
COPY libraries/agent-api/agent-api-state ./libraries/agent-api/agent-api-state
COPY libraries/agent-api/agent-api-workspace ./libraries/agent-api/agent-api-workspace
COPY libraries/agent-api/exec-fs ./libraries/agent-api/exec-fs
COPY libraries/agent-api/exec-git ./libraries/agent-api/exec-git
COPY libraries/agent-api/exec-hosted ./libraries/agent-api/exec-hosted
COPY libraries/agent-api/exec-proc ./libraries/agent-api/exec-proc
COPY libraries/agent-api/exec-proto ./libraries/agent-api/exec-proto
# The test kit is a dev-dependency cargo resolves but the build never compiles: its manifest and an empty library are all it needs here.
COPY libraries/agent-api/exec-testkit/Cargo.toml ./libraries/agent-api/exec-testkit/Cargo.toml
RUN mkdir -p libraries/agent-api/exec-testkit/src && : > libraries/agent-api/exec-testkit/src/lib.rs
WORKDIR /src/clis
RUN sed -i 's|^members = .*|members = ["wf-kit", "orca-serve", "orca-serve/crypto", "orca-serve/core", "orca-serve/map-settings", "orca-serve/map-mobile", "orca-serve/map-workspace", "orca-serve/map-agent", "orca-serve/map-terminal", "orca-serve/map-git", "orca-serve/map-accounts", "orca-serve/map-state", "orca-serve/map-hosted", "orca-serve/map-schedules"]|' Cargo.toml
# --bin: the recipe keeps this binary's crates only, so a sibling's manifest does not move it.
RUN --mount=type=cache,id=cargo-registry-orca-serve,target=/usr/local/cargo/registry \
    cargo chef prepare --recipe-path /recipe.json --bin orca-serve
# The contract crates as the dependency layer sees them: each manifest and an empty library. Cargo needs their paths to exist, but the recipe
# does not hold them, so an edit to their sources must not reach the layer below.
RUN set -eu; for crate in /src/libraries/agent-api/*/; do \
      name="$(basename "$crate")"; mkdir -p "/skel/$name/src"; cp "$crate/Cargo.toml" "/skel/$name/Cargo.toml"; : > "/skel/$name/src/lib.rs"; \
    done

FROM chef AS builder
WORKDIR /src/clis
COPY --from=planner /recipe.json /recipe.json
COPY --from=planner /skel /src/libraries/agent-api
# No --locked: the recipe's lockfile is cut to this binary's packages; the build below checks the real one.
RUN --mount=type=cache,id=cargo-registry-orca-serve,target=/usr/local/cargo/registry \
    cargo chef cook --release --recipe-path /recipe.json -p orca-serve
COPY --from=planner /src /src
# Every source must be newer than the stub the cook layer compiled, or cargo keeps the stub.
RUN --mount=type=cache,id=cargo-registry-orca-serve,target=/usr/local/cargo/registry \
    find /src/clis /src/libraries -not -path '/src/clis/target*' -name '*.rs' -exec touch {} + \
    && cargo build --release --locked -p orca-serve --bin orca-serve \
    && cp target/release/orca-serve /orca-serve

FROM debian:bookworm-slim AS runtime
COPY --from=aptcerts /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=builder /orca-serve /usr/local/bin/orca-serve
ENV SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt \
    ORCA_BIND=0.0.0.0 \
    ORCA_PORT=6768 \
    ORCA_STATE_DIR=/data
ARG GIT_COMMIT=unknown
ENV GIT_COMMIT=$GIT_COMMIT
EXPOSE 6768
CMD ["orca-serve", "serve"]
