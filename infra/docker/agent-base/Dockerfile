# syntax=docker/dockerfile:1
#
# The shared foundation for every image that runs Node in this repo's VM stack:
# remote-chat-agent, opencode-server, and agent-shell.
#
# WHY THIS EXISTS. Two problems, only one of which is duplication.
#
# 1. VERSION DRIFT, which is the one that bites. agent-shell built from
#    `node:22-bookworm-slim` (floating) while remote-chat-agent and
#    opencode-server pinned `node:22.14.0-slim`. So the Node running an agent
#    CLI over SSH could silently differ from the Node running the same CLI for
#    Telegram — the exact "two surfaces that look alike but aren't" failure the
#    shared-workspace work exists to eliminate. A shared base makes that
#    impossible rather than merely unlikely.
#
# 2. The apt-over-HTTPS + CA bootstrap below was copy-pasted into two of the
#    three Dockerfiles and absent from the third. It is fiddly, load-bearing
#    behind a CONNECT-only proxy, and exactly the kind of thing that rots when
#    it lives in three places.
#
# WHAT IT DELIBERATELY DOES NOT CARRY. The agent CLIs themselves (claude, codex,
# opencode) are NOT here. remote-chat-agent prunes its set at build time via
# AGENT_CLIS so an opencode-only deploy does not ship a ~263 MB bundled claude
# executable, and baking them into the base would defeat that. Playwright,
# Docker Engine and sshd stay in agent-shell, which is the only image that needs
# them.

# Source of the CA bundle used to switch apt to HTTPS. The slim base ships no
# certs, and installing ca-certificates would itself need working apt — so the
# bundle is copied from the FULL node image of the same version: identical
# Debian release, identical certs, no network needed to bootstrap it.
FROM node:22.14.0 AS aptcerts

FROM node:22.14.0-slim

# ── apt over HTTPS ───────────────────────────────────────────────────────────
# Debian's default sources are plain `http://`, which cannot work behind a proxy
# that only permits HTTPS CONNECT — this repo's agent sandbox answers 405 to
# every non-CONNECT request, so `apt-get update` fails with "Method Not Allowed"
# and no image here could be built from a session. Harmless everywhere else.
COPY --from=aptcerts /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
RUN sed -i 's|http://deb.debian.org|https://deb.debian.org|g; s|http://security.debian.org|https://security.debian.org|g' \
      /etc/apt/sources.list.d/debian.sources 2>/dev/null || true; \
    sed -i 's|http://deb.debian.org|https://deb.debian.org|g; s|http://security.debian.org|https://security.debian.org|g' \
      /etc/apt/sources.list 2>/dev/null || true

# Optional trust for a TLS-terminating egress proxy's CA, passed as a BuildKit
# secret and never committed. A no-op when absent, which is the CI and release
# case — so no published image ever trusts an extra CA.
RUN --mount=type=secret,id=proxy_ca \
    if [ -s /run/secrets/proxy_ca ]; then \
      apt-get update && apt-get install -y --no-install-recommends ca-certificates && \
      cp /run/secrets/proxy_ca /usr/local/share/ca-certificates/egress-proxy.crt && \
      update-ca-certificates && \
      rm -rf /var/lib/apt/lists/*; \
    fi

# git: every consumer needs it — worktree clones in the agent, the working tree
# opencode inspects, and interactive use in agent-shell. curl: fetching release
# tarballs at build time and general use at runtime.
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt/lists,sharing=locked \
    apt-get update && apt-get install -y --no-install-recommends \
      git ca-certificates curl unzip

# pnpm via npm, NOT corepack. corepack does not honor HTTP_PROXY/HTTPS_PROXY (it
# fetches through undici with no proxy support until Node's --use-env-proxy,
# which landed after 22.14), so `corepack prepare` fails outright behind a proxy
# — which is every build inside this repo's agent sandbox. `npm i -g pnpm`
# reaches the same registry through the proxy npm already uses.
RUN npm install -g pnpm@11.5.2

# bws CLI (Bitwarden Secrets Manager). Here for the same reason gh is: TWO of
# the three consumers need it, and were asymmetric about it in opposite
# directions -- agent-shell had bws and no gh, remote-chat-agent had gh and no
# bws. Both were measured with toolchain-check, not assumed.
#
# remote-chat-agent's need is not hypothetical: its compose forwards
# BWS_ADMIN_ACCESS_TOKEN and BWS_SERVER_URL specifically so
# service_kit::secrets (through wf) can resolve ANY registered secret at runtime --
# and wf's bitwarden backend resolves by spawning this binary. Credentials forwarded to a container with no binary to
# spend them is a no-op that reports "not set" rather than "could not check".
#
# The version comes from mise.toml, which already pins this exact tool for every
# other surface (`"ubi:bitwarden/sdk-sm[exe=bws]" = "bws-v2.1.0"`).
#
# Do NOT reintroduce version discovery via api.github.com/repos/bitwarden/sdk-sm
# /releases. bws ships on an independent `bws-v*` train so GitHub's own
# /releases/latest points at the wrong thing, but the API call cannot work from
# an agent sandbox at all: the egress proxy scopes api.github.com/repos/* to the
# repositories enabled for the session and 403s every other repo. The ASSET host
# (github.com/.../releases/download/...) answers 200 for the same repo, which is
# why this looked like a flaky download rather than a blocked API. agent-shell
# shipped with no bws for as long as nobody looked, because the step was
# best-effort and swallowed that 403.
COPY mise.toml /tmp/mise.toml
RUN set -eux; \
    BWS_TAG="$(grep -oE 'bws-v[0-9]+\.[0-9]+\.[0-9]+' /tmp/mise.toml | head -1)"; \
    test -n "$BWS_TAG"; \
    BWS_VERSION="${BWS_TAG#bws-v}"; \
    case "$(uname -m)" in \
      aarch64) triple=aarch64-unknown-linux-gnu ;; \
      x86_64)  triple=x86_64-unknown-linux-gnu ;; \
      *) echo "unsupported arch $(uname -m)"; exit 1 ;; \
    esac; \
    curl -fsSL "https://github.com/bitwarden/sdk-sm/releases/download/${BWS_TAG}/bws-${triple}-${BWS_VERSION}.zip" -o /tmp/bws.zip; \
    unzip -o /tmp/bws.zip -d /usr/local/bin; \
    chmod +x /usr/local/bin/bws; \
    rm /tmp/bws.zip /tmp/mise.toml; \
    bws --version

# gh CLI. Here rather than in one consumer because TWO of the three need it and
# were asymmetric about it: remote-chat-agent shipped it for the coding sessions
# it spawns inside worktrees (PR creation, CI status, review comments), while
# agent-shell -- a human working on THE SAME worktrees over SSH -- had no gh at
# all. Measured, not assumed: toolchain-check reported `gh 2.96.0` on one and
# `MISSING` on the other. That is the two-surfaces-that-look-alike failure this
# base exists to remove, so it belongs here rather than being pasted into a
# second Dockerfile.
#
# From GitHub's own release tarball, not apt: Debian slim's repos don't carry
# gh, and adding the apt.github.com source just to avoid an arch case-split
# isn't worth the extra apt-key/repo-trust surface.
#
# Pinned literal, deliberately NOT read from mise.toml the way the bws pin in
# agent-shell is: mise.toml says `gh = "latest"`, which is the floating-tag
# problem this base was created to fix, so sourcing it there would reintroduce
# the drift under the appearance of having one source of truth.
ARG GH_CLI_VERSION=2.96.0
RUN set -eux; \
    case "$(uname -m)" in \
      x86_64)  a=amd64 ;; \
      aarch64) a=arm64 ;; \
      *) echo "unsupported arch $(uname -m)"; exit 1 ;; \
    esac; \
    curl -fsSL "https://github.com/cli/cli/releases/download/v${GH_CLI_VERSION}/gh_${GH_CLI_VERSION}_linux_${a}.tar.gz" -o /tmp/gh.tgz; \
    tar -xzf /tmp/gh.tgz -C /tmp; \
    install -m 0755 "/tmp/gh_${GH_CLI_VERSION}_linux_${a}/bin/gh" /usr/local/bin/gh; \
    rm -rf /tmp/gh.tgz "/tmp/gh_${GH_CLI_VERSION}_linux_${a}"; \
    gh --version

# Scopes the root postinstall hook to only what an image needs: a filtered
# install never has unrelated workspace CLIs' devDependencies present, so the
# full dev chain fails trying to build them.
ENV DOCKER_BUILD=true
