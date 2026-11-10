# syntax=docker/dockerfile:1
#
# Built from the REPO ROOT context (see .github/workflows/deploy.yml): the Rust workspace
# manifest lives in backend/ and the crate reads files from beside it. Built natively for the image's architecture by
# the deploy leg of CI (`wf ship build remote-chat-agent --tag <tag>`).
#
# The service is one Rust binary (backend/remote-chat-agent/rs, crate `rca-service`), not a Node process. What the
# runtime image carries is what that binary spawns:
#   - git and gh: the agent clones public repos anonymously and creates worktrees on demand; the service holds no GitHub
#     credential, so gh is unauthenticated.
#   - pnpm (and the Node it needs): `pnpm install` in every worktree the service creates, and `wf vm deps
#     replay` at boot. Node is a tool here and never runs the service.
#   - ffmpeg and whisper.cpp: speech to text when no sidecar is configured. poppler-utils and tesseract: documents and
#     pictures attached to a chat turn. sqlite3: the recent-models list read from opencode's own database.
# Toolchain (Node 22.14.0, pnpm 11.5.2), the apt-over-HTTPS bootstrap, the optional egress-proxy CA secret, git, curl and
# gh come from the shared agent-base image (infra/docker/agent-base/Dockerfile), so this service and the SSH surface
# cannot drift apart.
#
# Declared before the FIRST FROM so they are global build args; an ARG after a FROM is scoped to that stage only.
ARG AGENT_BASE_TAG=latest
ARG IMAGE_NAMESPACE=nuesslerm

# Dependencies first: cargo-chef compiles what the manifests and the lockfile name into an image layer, which `mode=max`
# exports and a recreated VM imports; a source change then rebuilds the workspace crates alone.
FROM docker.io/library/rust:1-bookworm AS chef
RUN --mount=type=cache,id=cargo-registry-remote-chat-agent,target=/usr/local/cargo/registry \
    cargo install cargo-chef --locked --version 0.1.78
WORKDIR /src
# The shared libraries come from the mla registry (libs-proxy on the tailnet); the lockfile pins their versions.
COPY .cargo/config.toml /src/.cargo/config.toml

# The service itself: its own workspace; the shared libraries (service-kit, telemetry, rca-core, rca-ladder) are registry
# dependencies, not copied; the files the crates embed (`include_str!`/`include_bytes!`) are copied to the paths they name
# relative to the sources. The wire contracts (libraries/agent-api, a workspace of their own that the service reaches by path) are
# copied one crate at a time: `wf` hashes the image from the crates its Dockerfile names, and a whole-directory COPY would be dropped
# from the hash because that directory holds a workspace manifest. Their tests are not in the context (.dockerignore).
FROM chef AS planner
COPY backend/remote-chat-agent/Cargo.toml backend/remote-chat-agent/Cargo.lock ./backend/remote-chat-agent/
COPY backend/remote-chat-agent/core ./backend/remote-chat-agent/core
COPY backend/remote-chat-agent/cli-driver ./backend/remote-chat-agent/cli-driver
COPY backend/remote-chat-agent/exec-channel ./backend/remote-chat-agent/exec-channel
COPY backend/remote-chat-agent/agent-core ./backend/remote-chat-agent/agent-core
COPY backend/remote-chat-agent/group-agent ./backend/remote-chat-agent/group-agent
COPY backend/remote-chat-agent/group-workspace ./backend/remote-chat-agent/group-workspace
COPY backend/remote-chat-agent/group-state ./backend/remote-chat-agent/group-state
COPY backend/remote-chat-agent/group-accounts ./backend/remote-chat-agent/group-accounts
COPY backend/remote-chat-agent/group-terminal ./backend/remote-chat-agent/group-terminal
COPY backend/remote-chat-agent/group-git ./backend/remote-chat-agent/group-git
COPY backend/remote-chat-agent/group-hosted ./backend/remote-chat-agent/group-hosted
COPY backend/remote-chat-agent/group-schedules ./backend/remote-chat-agent/group-schedules
COPY backend/remote-chat-agent/group-attachments ./backend/remote-chat-agent/group-attachments
COPY backend/remote-chat-agent/group-catalog ./backend/remote-chat-agent/group-catalog
COPY backend/remote-chat-agent/group-preview ./backend/remote-chat-agent/group-preview
COPY backend/remote-chat-agent/service-core ./backend/remote-chat-agent/service-core
COPY backend/remote-chat-agent/render ./backend/remote-chat-agent/render
COPY backend/remote-chat-agent/opencode ./backend/remote-chat-agent/opencode
COPY backend/remote-chat-agent/speech ./backend/remote-chat-agent/speech
COPY backend/remote-chat-agent/host ./backend/remote-chat-agent/host
COPY backend/remote-chat-agent/rs ./backend/remote-chat-agent/rs
COPY libraries/agent-api/agent-api-proto ./libraries/agent-api/agent-api-proto
COPY libraries/agent-api/agent-api-service ./libraries/agent-api/agent-api-service
COPY libraries/agent-api/agent-api-state ./libraries/agent-api/agent-api-state
COPY libraries/agent-api/agent-api-workspace ./libraries/agent-api/agent-api-workspace
COPY libraries/agent-api/agent-api-agent ./libraries/agent-api/agent-api-agent
COPY libraries/agent-api/agent-api-terminal ./libraries/agent-api/agent-api-terminal
COPY libraries/agent-api/agent-api-git ./libraries/agent-api/agent-api-git
COPY libraries/agent-api/agent-api-hosted ./libraries/agent-api/agent-api-hosted
COPY libraries/agent-api/agent-api-schedules ./libraries/agent-api/agent-api-schedules
COPY libraries/agent-api/agent-api-attachments ./libraries/agent-api/agent-api-attachments
COPY libraries/agent-api/agent-api-catalog ./libraries/agent-api/agent-api-catalog
COPY libraries/agent-api/agent-api-preview ./libraries/agent-api/agent-api-preview
COPY libraries/agent-api/agent-api-accounts ./libraries/agent-api/agent-api-accounts
COPY libraries/agent-api/exec-proto ./libraries/agent-api/exec-proto
COPY libraries/agent-api/exec-fs ./libraries/agent-api/exec-fs
COPY libraries/agent-api/exec-proc ./libraries/agent-api/exec-proc
COPY libraries/agent-api/exec-git ./libraries/agent-api/exec-git
COPY libraries/agent-api/exec-hosted ./libraries/agent-api/exec-hosted
COPY libraries/agent-api/exec-preview ./libraries/agent-api/exec-preview
COPY libraries/agent-api/exec-pty ./libraries/agent-api/exec-pty
# The test kit is a dev-dependency of the service, which cargo resolves though it never builds it: its manifest is the whole of it here, so an
# edit to its code or recordings is in no image. Cargo refuses a path dependency that names no target, so it gets an empty library.
COPY libraries/agent-api/exec-testkit/Cargo.toml ./libraries/agent-api/exec-testkit/Cargo.toml
RUN mkdir -p libraries/agent-api/exec-testkit/src && : > libraries/agent-api/exec-testkit/src/lib.rs
COPY frontend/mla-tamagui/assets/fonts/NotoSansTamil-Regular.ttf ./frontend/mla-tamagui/assets/fonts/NotoSansTamil-Regular.ttf
WORKDIR /src/backend/remote-chat-agent
# --bin: the recipe keeps this service's crates only, so a sibling's manifest does not move it.
RUN --mount=type=cache,id=cargo-registry-remote-chat-agent,target=/usr/local/cargo/registry \
    cargo chef prepare --recipe-path /recipe.json --bin rca-service
# The contract crates as the dependency layer sees them: each manifest and an empty library. Cargo needs their paths to exist, but the
# recipe does not hold them, so an edit to their sources must not reach the layer below.
RUN set -eu; for crate in /src/libraries/agent-api/*/; do \
      name="$(basename "$crate")"; mkdir -p "/skel/$name/src"; cp "$crate/Cargo.toml" "/skel/$name/Cargo.toml"; : > "/skel/$name/src/lib.rs"; \
    done

FROM chef AS rcabuilder
WORKDIR /src/backend/remote-chat-agent
COPY --from=planner /recipe.json /recipe.json
COPY --from=planner /skel /src/libraries/agent-api
# No --locked: the recipe's lockfile is cut to this service's packages; the build below checks the real one.
RUN --mount=type=cache,id=cargo-registry-remote-chat-agent,target=/usr/local/cargo/registry \
    cargo chef cook --recipe-path /recipe.json --profile deploy -p rca-service
COPY --from=planner /src /src
# Every source must be newer than the stub the cook layer compiled, or cargo keeps the stub.
RUN --mount=type=cache,id=cargo-registry-remote-chat-agent,target=/usr/local/cargo/registry \
    find /src/backend /src/libraries -not -path '/src/backend/remote-chat-agent/target*' -name '*.rs' -exec touch {} + \
    && cargo build --profile deploy --locked -p rca-service \
    && cp target/deploy/rca-service /rca-service

FROM docker.io/${IMAGE_NAMESPACE:-nuesslerm}/agent-base:${AGENT_BASE_TAG} AS base
WORKDIR /repo

# ── whisper stage: whisper.cpp + models, rebuilt only when the pinned package version changes ──
# Independent of the app sources, so a code change never recompiles it or re-downloads the models. The package is used
# for its vendored whisper.cpp sources only; nothing of it runs.
FROM base AS whisper
ARG WHISPER_NODE_VERSION=1.1.1
# A named CPU, not -mcpu=native: the binary must not depend on whatever host built it (Ampere Altra, Neoverse-N1).
ARG WHISPER_MCPU=neoverse-n1
ARG WHISPER_MODELS="base.en tiny.en"
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt/lists,sharing=locked \
    apt-get update && apt-get install -y --no-install-recommends make g++ ca-certificates
RUN --mount=type=secret,id=proxy_ca \
    if [ -s /run/secrets/proxy_ca ]; then export NODE_EXTRA_CA_CERTS=/run/secrets/proxy_ca; fi; \
    mkdir /pkg && cd /pkg && npm pack "whisper-node@${WHISPER_NODE_VERSION}" --silent \
    && tar xzf whisper-node-*.tgz --strip-components=1 \
    && cd lib/whisper.cpp \
    && sed -i "s/-mcpu=native/-mcpu=${WHISPER_MCPU}/g" Makefile \
    && make main -j"$(nproc)" \
    && for model in ${WHISPER_MODELS}; do bash ./models/download-ggml-model.sh "$model"; done

# ── ffmpeg stage: one static binary, checksum-pinned ──
# Debian's `ffmpeg` package drags in ~200 packages and 350 MB (mesa, llvm, SDL, flite, ...) for a tool that here only
# converts an upload to 16 kHz mono WAV. This is the static build the Node image's `ffmpeg-static` package downloaded
# (the same release, so the same provenance), pinned by its SHA-256 for the architecture being built.
FROM base AS ffmpeg
ARG FFMPEG_STATIC_RELEASE=b6.0
ARG FFMPEG_STATIC_SHA256_ARM64=2b708b2d15041d2a192c1db24c7a8a1d24f645a8242dce1c744ff2392b86ada1
ARG FFMPEG_STATIC_SHA256_X64=17c1ae10b52ac499180679fe6ba77e17642390c4eedb0f1e3b0ac045da55128f
RUN --mount=type=secret,id=proxy_ca \
    set -eux; \
    case "$(uname -m)" in \
      aarch64) arch=arm64; sha="$FFMPEG_STATIC_SHA256_ARM64" ;; \
      x86_64)  arch=x64;   sha="$FFMPEG_STATIC_SHA256_X64" ;; \
      *) echo "unsupported arch $(uname -m)"; exit 1 ;; \
    esac; \
    if [ -s /run/secrets/proxy_ca ]; then export CURL_CA_BUNDLE=/run/secrets/proxy_ca; fi; \
    curl -fsSL "https://github.com/eugeneware/ffmpeg-static/releases/download/${FFMPEG_STATIC_RELEASE}/ffmpeg-linux-${arch}.gz" -o /tmp/ffmpeg.gz; \
    echo "${sha}  /tmp/ffmpeg.gz" | sha256sum -c -; \
    gunzip -c /tmp/ffmpeg.gz > /ffmpeg; \
    chmod 755 /ffmpeg; \
    /ffmpeg -version | head -1

# ── runtime stage ──
FROM base AS runtime
# git (on-demand worktree clones) and ca-certificates (the service's HTTP client trusts the system roots) come from the
# shared base; gh comes from it too, pinned there so the SSH surface working on the same worktrees has the same tool. This
# service gives it no token.
# Non-root runtime identity, uid 1001 to match agent-shell's `agent` exactly -- the shared agent-workspace volume is owned by
# ONE uid, and a mismatch here is what makes a worktree this service creates unmodifiable from the SSH surface
# (docs/solutions/infra/sharing-a-docker-volume-across-containers-needs-one-uid.md). 1001 rather than 1000 because the base
# already ships `node` at 1000. `runuser` (util-linux) is what docker-entrypoint.sh drops privileges with.
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt/lists,sharing=locked \
    apt-get update && apt-get install -y --no-install-recommends \
      util-linux poppler-utils tesseract-ocr tesseract-ocr-eng sqlite3 \
    && useradd -m -u 1001 -s /bin/bash agent
# The SAME script agent-shell ships -- shared deliberately rather than reimplemented, so the two surfaces cannot drift on
# retry behaviour, host resolution, or error messages. A thin wrapper over vm-ops-api's own POST /services/:app/redeploy: no
# docker socket here either, and it can only select an already-published tag.
COPY infra/oracle-cloud/agent-shell/redeploy-app.sh /usr/local/bin/redeploy-app
RUN chmod 755 /usr/local/bin/redeploy-app
# The wf CLI is the VM's one build on the `clis` volume (clis.yml), mounted at /opt/clis: `wf vm deps replay` runs from
# docker-entrypoint.sh on every boot, and the service asks `wf secret env` for any registered secret compose did not pass.
RUN ln -sf /opt/clis/current/wf /usr/local/bin/wf
COPY backend/remote-chat-agent/docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh
COPY --from=whisper /pkg/lib/whisper.cpp /opt/whisper.cpp
COPY --from=ffmpeg /ffmpeg /usr/local/bin/ffmpeg
COPY --from=rcabuilder /rca-service /usr/local/bin/rca-service
# whisper.cpp is the speech-to-text fallback when STT_URL names no sidecar; its model is found beside the binary
# (models/ggml-<WHISPER_MODEL_NAME>.bin). ffmpeg, pdftotext and tesseract are found on PATH.
ENV WHISPER_CPP_BINARY=/opt/whisper.cpp/main
WORKDIR /app
# Declared last: an ENV that changes with every commit would otherwise invalidate every layer after it.
ARG GIT_COMMIT=unknown
ENV GIT_COMMIT=$GIT_COMMIT
EXPOSE 8787
ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
CMD ["/usr/local/bin/rca-service"]
