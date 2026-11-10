# syntax=docker/dockerfile:1
#
# Built from the REPO ROOT context (the service's Cargo workspace is backend/libs-proxy). The service is one Rust binary
# (backend/libs-proxy/rs); the runtime image holds nothing else but a CA bundle for the HTTPS reads of the bucket.

# Source of the CA bundle the runtime carries.
FROM alpine:3.20 AS aptcerts
RUN apk add --no-cache ca-certificates

# Dependencies first: cargo-chef compiles what the manifests and the lockfile name into an image layer, which `mode=max`
# exports and a recreated VM imports; a source change then rebuilds the workspace crates alone.
FROM docker.io/library/rust:1-bookworm AS chef
RUN --mount=type=cache,id=cargo-registry-libs-proxy,target=/usr/local/cargo/registry \
    cargo install cargo-chef --locked --version 0.1.78
WORKDIR /src

FROM chef AS planner
# This service is its own Cargo workspace with crates.io dependencies only: nothing else is copied, so no other service or library
# can move this image. The manifest and lockfile are copied as files, not as a directory, so the image hash reads them
# (clis/pipeline/src/image_hash.rs).
COPY backend/libs-proxy/Cargo.toml backend/libs-proxy/Cargo.lock ./backend/libs-proxy/
COPY backend/libs-proxy/core ./backend/libs-proxy/core
COPY backend/libs-proxy/rs ./backend/libs-proxy/rs
WORKDIR /src/backend/libs-proxy
RUN --mount=type=cache,id=cargo-registry-libs-proxy,target=/usr/local/cargo/registry \
    cargo chef prepare --recipe-path /recipe.json --bin libs-proxy-rs

FROM chef AS builder
WORKDIR /src/backend/libs-proxy
COPY --from=planner /recipe.json /recipe.json
# No --locked: the recipe's lockfile is this service's own; the build below checks the real one.
RUN --mount=type=cache,id=cargo-registry-libs-proxy,target=/usr/local/cargo/registry \
    cargo chef cook --recipe-path /recipe.json --release -p libs-proxy-rs
COPY --from=planner /src /src
# Every source must be newer than the stub the cook layer compiled, or cargo keeps the stub.
RUN --mount=type=cache,id=cargo-registry-libs-proxy,target=/usr/local/cargo/registry \
    find /src/backend/libs-proxy -not -path '/src/backend/libs-proxy/target*' -name '*.rs' -exec touch {} + \
    && cargo build --release --locked -p libs-proxy-rs \
    && cp target/release/libs-proxy-rs /libs-proxy-rs

FROM debian:bookworm-slim AS runtime
COPY --from=aptcerts /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=builder /libs-proxy-rs /usr/local/bin/libs-proxy-rs
ENV SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt
ARG GIT_COMMIT=unknown
ENV GIT_COMMIT=$GIT_COMMIT
EXPOSE 8793
CMD ["libs-proxy-rs"]
