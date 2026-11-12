# syntax=docker/dockerfile:1
#
# Built from the REPO ROOT context (the Rust workspace manifest lives in backend/): the control surface of the VM as one Rust binary
# (backend/vm-ops-api/rs). The runtime is its own helper: the redeploy siblings `docker run` this image on the
# plain bridge network to run `docker compose`, `wf` and the `vm-ops-api-rs` subcommands, so it keeps glibc (the host's `bws` is
# glibc-linked), the CA store (without it `bws` panics), the docker CLI with its compose plugin, `sh` and the coreutils.
# Never add an ENTRYPOINT: a resolv.conf-rewriting one breaks those siblings: they run on the plain bridge network, where the
# tailnet's resolver is unreachable.

FROM docker:26-cli AS dockercli

# Dependencies first: cargo-chef compiles what the manifests and the lockfile name into an image layer, which `mode=max`
# exports and a recreated VM imports; a source change then rebuilds the workspace crates alone.
FROM docker.io/library/rust:1-bookworm AS chef
RUN --mount=type=cache,id=cargo-registry-vm-ops-api,target=/usr/local/cargo/registry \
    cargo install cargo-chef --locked --version 0.1.78
WORKDIR /src
# The shared libraries come from the mla registry (a public bucket, plain HTTPS); the lockfile pins their versions.
COPY .cargo/config.toml /src/.cargo/config.toml

FROM chef AS planner
# This service is its own Cargo workspace (backend/vm-ops-api/Cargo.toml and Cargo.lock): no sibling service is copied, and a dependency change
# in another service cannot move this image. The manifest and lockfile are copied as files so the image hash reads them (clis/pipeline/src/image_hash.rs).
COPY backend/vm-ops-api/Cargo.toml backend/vm-ops-api/Cargo.lock ./backend/vm-ops-api/
COPY backend/vm-ops-api/core ./backend/vm-ops-api/core
COPY backend/vm-ops-api/rs ./backend/vm-ops-api/rs
WORKDIR /src/backend/vm-ops-api
# --bin: the recipe keeps this service's crates only, so a sibling's manifest does not move it.
RUN --mount=type=cache,id=cargo-registry-vm-ops-api,target=/usr/local/cargo/registry \
    cargo chef prepare --recipe-path /recipe.json --bin vm-ops-api-rs

FROM chef AS builder
WORKDIR /src/backend/vm-ops-api
COPY --from=planner /recipe.json /recipe.json
# No --locked: the recipe's lockfile is cut to this service's packages; the build below checks the real one.
RUN --mount=type=cache,id=cargo-registry-vm-ops-api,target=/usr/local/cargo/registry \
    cargo chef cook --recipe-path /recipe.json --profile deploy -p vm-ops-api-rs
COPY --from=planner /src /src
# Every source must be newer than the stub the cook layer compiled, or cargo keeps the stub.
RUN --mount=type=cache,id=cargo-registry-vm-ops-api,target=/usr/local/cargo/registry \
    find /src/backend -not -path '/src/backend/vm-ops-api/target*' -name '*.rs' -exec touch {} + \
    && cargo build --profile deploy --locked -p vm-ops-api-rs \
    && cp target/deploy/vm-ops-api-rs /vm-ops-api-rs

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates \
  && update-ca-certificates \
  && rm -rf /var/lib/apt/lists/*
# docker CLI and compose v2 plugin: static Go binaries, so copying them into this glibc base carries no libc risk.
COPY --from=dockercli /usr/local/bin/docker /usr/local/bin/docker
COPY --from=dockercli /usr/local/libexec/docker/cli-plugins/docker-compose /usr/local/libexec/docker/cli-plugins/docker-compose
# wf: the VM's one build on the `clis` volume (clis.yml), mounted at /opt/clis; also the logger (`wf log emit`).
RUN ln -sf /opt/clis/current/wf /usr/local/bin/wf
COPY --from=builder /vm-ops-api-rs /usr/local/bin/vm-ops-api-rs
ARG GIT_COMMIT=unknown
ENV GIT_COMMIT=$GIT_COMMIT
CMD ["vm-ops-api-rs"]
