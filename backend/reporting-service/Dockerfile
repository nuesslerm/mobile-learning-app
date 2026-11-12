# syntax=docker/dockerfile:1
#
# Built from the REPO ROOT context (the service's Cargo workspace is backend/reporting-service). The service is one Rust binary
# (backend/reporting-service/rs) the runtime image holds nothing else but a CA bundle for the telemetry export.

# Source of the CA bundle the runtime carries.
FROM alpine:3.20 AS aptcerts
RUN apk add --no-cache ca-certificates

# Dependencies first: cargo-chef compiles what the manifests and the lockfile name into an image layer, which `mode=max`
# exports and a recreated VM imports; a source change then rebuilds the workspace crates alone.
FROM docker.io/library/rust:1-bookworm AS chef
RUN --mount=type=cache,id=cargo-registry-reporting-service,target=/usr/local/cargo/registry \
    cargo install cargo-chef --locked --version 0.1.78
WORKDIR /src
# The shared libraries come from the mla registry (a public bucket, plain HTTPS); the lockfile pins their versions.
COPY .cargo/config.toml /src/.cargo/config.toml

FROM chef AS planner
# This service is its own Cargo workspace (backend/reporting-service/Cargo.toml and Cargo.lock): no sibling service is copied, and a
# dependency change in another service cannot move this image. The manifest and lockfile are copied as files, not as a directory, so
# the image hash reads them (clis/pipeline/src/image_hash.rs).
COPY backend/reporting-service/Cargo.toml backend/reporting-service/Cargo.lock ./backend/reporting-service/
COPY backend/reporting-service/core ./backend/reporting-service/core
COPY backend/reporting-service/rs ./backend/reporting-service/rs
WORKDIR /src/backend/reporting-service
RUN --mount=type=cache,id=cargo-registry-reporting-service,target=/usr/local/cargo/registry \
    cargo chef prepare --recipe-path /recipe.json --bin reporting-service-rs

FROM chef AS builder
WORKDIR /src/backend/reporting-service
COPY --from=planner /recipe.json /recipe.json
# No --locked: the recipe's lockfile is this service's own; the build below checks the real one.
RUN --mount=type=cache,id=cargo-registry-reporting-service,target=/usr/local/cargo/registry \
    cargo chef cook --recipe-path /recipe.json --profile deploy -p reporting-service-rs
COPY --from=planner /src /src
# Every source must be newer than the stub the cook layer compiled, or cargo keeps the stub.
RUN --mount=type=cache,id=cargo-registry-reporting-service,target=/usr/local/cargo/registry \
    find /src/backend -not -path '/src/backend/reporting-service/target*' -name '*.rs' -exec touch {} + \
    && cargo build --profile deploy --locked -p reporting-service-rs \
    && cp target/deploy/reporting-service-rs /reporting-service-rs

FROM debian:bookworm-slim AS runtime
COPY --from=aptcerts /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=builder /reporting-service-rs /usr/local/bin/reporting-service-rs
ENV SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt
ARG GIT_COMMIT=unknown
ENV GIT_COMMIT=$GIT_COMMIT
EXPOSE 8791
CMD ["reporting-service-rs"]
