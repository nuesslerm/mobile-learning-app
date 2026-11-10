# Carrier image for the static `logger` binary (clis/telemetry's `logger` bin, the same line as `wf log emit`), extracted
# onto the host by cloud-init via `docker cp`. vm-ops-api and agent-shell call the VM's wf instead. Build from the repo
# root with docker buildx in agent-shell's own docker and push by hand (CI's deploy legs build no logger-cli).
FROM docker.io/library/rust:1-alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY clis/telemetry ./clis/telemetry
RUN --mount=type=cache,id=cargo-registry-logger-cli,target=/usr/local/cargo/registry \
    cargo build --release --manifest-path clis/telemetry/Cargo.toml --features cli --bin logger \
    && cp clis/telemetry/target/release/logger /logger-bin

FROM scratch
COPY --from=builder /logger-bin /logger-bin
CMD ["/logger-bin"]
