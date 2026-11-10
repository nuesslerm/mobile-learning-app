# syntax=docker/dockerfile:1
#
# `t3 serve`: the T3 mapper in front of rca's agent API, one Rust binary (`t3-serve`) built from the repo root context. Only the cargo closure
# of the binary is copied (wf-kit, the t3 crates, the agent-api contract crates), and the clis workspace is trimmed to those members so the
# other clis crates cannot move this image. The runtime holds the binary and a CA bundle.
FROM alpine:3.20 AS aptcerts
RUN apk add --no-cache ca-certificates

FROM docker.io/library/rust:1-bookworm AS builder
WORKDIR /src
# The shared libraries come from the mla registry (libs-proxy on the tailnet); the lockfile pins their versions.
COPY .cargo/config.toml /src/.cargo/config.toml
COPY clis/Cargo.toml clis/Cargo.lock ./clis/
COPY clis/wf-kit ./clis/wf-kit
COPY clis/t3-serve ./clis/t3-serve
# The wire contracts are copied one crate at a time: `wf` hashes the image from the crates its Dockerfile names, and a whole-directory COPY
# would be dropped from the hash because that directory holds a workspace manifest. Their tests are not in the context (.dockerignore).
COPY libraries/agent-api/agent-api-accounts ./libraries/agent-api/agent-api-accounts
COPY libraries/agent-api/agent-api-agent ./libraries/agent-api/agent-api-agent
COPY libraries/agent-api/agent-api-client ./libraries/agent-api/agent-api-client
COPY libraries/agent-api/agent-api-proto ./libraries/agent-api/agent-api-proto
COPY libraries/agent-api/agent-api-service ./libraries/agent-api/agent-api-service
COPY libraries/agent-api/agent-api-state ./libraries/agent-api/agent-api-state
COPY libraries/agent-api/agent-api-terminal ./libraries/agent-api/agent-api-terminal
COPY libraries/agent-api/agent-api-git ./libraries/agent-api/agent-api-git
COPY libraries/agent-api/agent-api-hosted ./libraries/agent-api/agent-api-hosted
COPY libraries/agent-api/agent-api-schedules ./libraries/agent-api/agent-api-schedules
COPY libraries/agent-api/agent-api-attachments ./libraries/agent-api/agent-api-attachments
COPY libraries/agent-api/agent-api-catalog ./libraries/agent-api/agent-api-catalog
COPY libraries/agent-api/agent-api-preview ./libraries/agent-api/agent-api-preview
COPY libraries/agent-api/agent-api-workspace ./libraries/agent-api/agent-api-workspace
COPY libraries/agent-api/exec-fs ./libraries/agent-api/exec-fs
COPY libraries/agent-api/exec-git ./libraries/agent-api/exec-git
COPY libraries/agent-api/exec-hosted ./libraries/agent-api/exec-hosted
COPY libraries/agent-api/exec-preview ./libraries/agent-api/exec-preview
COPY libraries/agent-api/exec-proc ./libraries/agent-api/exec-proc
COPY libraries/agent-api/exec-proto ./libraries/agent-api/exec-proto
# The test kit is a dev-dependency cargo resolves but the build never compiles: its manifest and an empty library are all it needs here.
COPY libraries/agent-api/exec-testkit/Cargo.toml ./libraries/agent-api/exec-testkit/Cargo.toml
RUN mkdir -p libraries/agent-api/exec-testkit/src && : > libraries/agent-api/exec-testkit/src/lib.rs
# The replay tests use mimic-core as a dev-dependency cargo resolves but the build never compiles: its manifest and an empty library are all it needs, so an edit to its sources rebuilds no image.
COPY clis/mimic/core/Cargo.toml ./clis/mimic/core/Cargo.toml
RUN mkdir -p clis/mimic/core/src && : > clis/mimic/core/src/lib.rs
WORKDIR /src/clis
RUN sed -i 's|^members = .*|members = ["wf-kit", "t3-serve", "t3-serve/contracts", "t3-serve/core", "t3-serve/store", "t3-serve/http"]|' Cargo.toml
RUN --mount=type=cache,id=cargo-registry-t3,target=/usr/local/cargo/registry \
    cargo build --release -p t3-serve --bin t3-serve \
    && cp target/release/t3-serve /t3-serve

FROM debian:bookworm-slim AS runtime
COPY --from=aptcerts /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=builder /t3-serve /usr/local/bin/t3-serve
ENV SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt \
    T3_BIND=0.0.0.0 \
    T3_PORT=8797 \
    T3_STATE_DIR=/data
ARG GIT_COMMIT=unknown
ENV GIT_COMMIT=$GIT_COMMIT
EXPOSE 8797
CMD ["t3-serve", "t3", "serve"]
