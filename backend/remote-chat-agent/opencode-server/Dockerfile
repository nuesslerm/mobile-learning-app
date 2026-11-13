# syntax=docker/dockerfile:1
#
# `opencode serve` — the dependency remote-chat-agent's opencode gateway talks
# to over HTTP, and which the deploy never provisioned.
#
# `config/agentClis.ts` registers opencode with `package: null` ("no heavy local
# dependency") because the app ships only `@opencode-ai/sdk`, an HTTP *client*.
# The gateway deliberately never spawns the server itself (see
# data/opencodeGateway.ts, and the Anthropic-credential policy in AGENTS.md) —
# so something else has to run it. Nothing did: the deployed agent dialled the
# default `http://localhost:4096`, found nothing, and every chat turn failed
# with `session_create: opencode session.create returned no session ID`.
#
# This image is that something. It joins remote-chat-agent's network namespace
# in docker-compose.prod.yml, so the agent's default `localhost:4096` resolves
# with no configuration change on either side.
#
# The app process still never spawns it — the CONTAINER supplies the dependency,
# which keeps the architectural property that gateway code has no child process
# whose env it would have to sanitize.

# Source of the CA bundle used to switch apt to HTTPS (see the base stage).
# Built FROM the shared base (infra/docker/agent-base/Dockerfile), which carries
# the pinned Node, the apt-over-HTTPS + CA bootstrap this file used to duplicate,
# git, and pnpm. The bootstrap in particular was copy-pasted here and into
# remote-chat-agent's Dockerfile, and absent from agent-shell's -- three copies
# of a fiddly, load-bearing step is how it rots.
ARG AGENT_BASE_TAG=latest
FROM docker.io/${IMAGE_NAMESPACE:-nuesslerm}/agent-base:${AGENT_BASE_TAG}

# the version the parity e2e tests of `wf offload ask` run `opencode run` at (clis/offload/tests/e2e): a run on this server behaves as they check
ARG OPENCODE_VERSION=1.19.0
RUN --mount=type=cache,target=/root/.npm npm install -g opencode-ai@${OPENCODE_VERSION}

# The directory opencode serves. remote-chat-agent passes an explicit `cwd` per
# turn for worktree-scoped sessions; this is only the default root.
ENV OPENCODE_WORKSPACE=/workspace
RUN mkdir -p /workspace
WORKDIR /workspace

EXPOSE 4096

# `--hostname 0.0.0.0` because the agent reaches this over the shared network
# namespace, not through the loopback interface of this container alone.
# Same non-root identity as the agent and as agent-shell's user -- opencode
# writes session state and worktree files onto the SHARED volume, and a uid
# mismatch there is precisely what makes those files unmodifiable from the
# other surface. 1001 because this base already ships `node` at 1000.
# `runuser` (util-linux) is how the entrypoint drops privileges.
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt/lists,sharing=locked \
    apt-get update && apt-get install -y --no-install-recommends util-linux \
    && useradd -m -u 1001 -s /bin/bash agent
# Provider options the server would otherwise run without. `setCacheKey`
# (the SDK's "Enable promptCacheKey for this provider", default false) is what
# lets a provider reuse the constant prefix every voice-edit turn sends -- the
# DSL system prompt -- instead of prefilling it from scratch each time. Off, a
# call whose prompt is 90% identical to the previous one pays for 100% of it.
# Named server-config.json rather than opencode.json because the repo-root
# opencode.json is a generated agent-side file and tooling refuses edits to
# anything by that name. Proof it is honoured is the provider's own
# `tokens.cache.read` count going non-zero from the second call, which
# remote-chat-agent now logs as `cacheRead` on every voice-edit turn.
COPY backend/remote-chat-agent/opencode-server/server-config.json /etc/opencode/server-config.json
ENV OPENCODE_CONFIG=/etc/opencode/server-config.json
COPY backend/remote-chat-agent/opencode-server/docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh
ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
CMD ["opencode", "serve", "--hostname", "0.0.0.0", "--port", "4096"]
# 13 Nov: touched on purpose to prove a gradual redeploy: only this image changes, so only the opencode sidecar is recreated.
# 13 Nov (2): touched again, after the smoke step stopped moving running sidecars: this change must recreate the opencode sidecar and nothing else.
