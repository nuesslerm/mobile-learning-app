# Isolated SSH surface for interactive/agent use of claude/codex/opencode/wf,
# deliberately separate from the VM host. The host's `ubuntu` user is in the
# `docker` group -- SSH access there is effectively near-root (docker-group
# membership is a well-known container-escape path). Redeploying the VM's
# OTHER apps still goes through vm-ops-api's own HTTP API (see
# entrypoint.sh's wf wrapper / README), never a local docker call against
# the host.
#
# This container DOES now run its own NESTED dockerd (Docker-in-Docker),
# entirely self-contained -- no /var/run/docker.sock mount, no host bind
# mount of any kind, matching exactly how a Claude Code Remote sandbox
# session gets Docker build+push capability (`dockerd &` inside the
# sandbox's OWN container, never a host socket). A fully compromised session here can build/push images and
# tear down its own nested Docker environment, but still cannot reach the
# host, the other apps' containers, or anything outside its own tailnet
# identity -- the property this container exists for is unchanged, only
# "no Docker capability at all" became "Docker capability fully contained
# to this one container's own nested daemon." See README.md for the
# `--privileged` tradeoff this requires and why it doesn't reopen the
# original host-escape path.
#
# Root build context: the COPY paths below are relative to the repository root.

# Declared before the FIRST `FROM` so they are GLOBAL build args. An ARG after a
# FROM belongs to that stage only -- placed next to the agent-base FROM below,
# both resolved to empty and the build died with `invalid reference format`.
ARG AGENT_BASE_TAG=latest
ARG IMAGE_NAMESPACE=nuesslerm


# Built FROM the shared base (infra/docker/agent-base/Dockerfile) rather than a
# floating node tag. This file used `node:22-bookworm-slim`, which resolves to
# whatever 22.x is current -- measured at v22.23.2 while remote-chat-agent and
# opencode-server sat on the pinned v22.14.0. NINE minor versions apart, so the
# Node running `claude` over SSH was not the Node running it for Telegram. That
# is the "two surfaces that look alike but aren't" failure this whole
# shared-workspace effort exists to remove, and no amount of shared storage
# fixes it.
#
# Both bases are bookworm (verified: VERSION_CODENAME=bookworm on each), so the
# hardcoded `debian bookworm stable` Docker CE repo line below stays valid.
FROM docker.io/${IMAGE_NAMESPACE:-nuesslerm}/agent-base:${AGENT_BASE_TAG}


# pnpm is NOT installed here -- it comes from the shared base, npm-installed and
# pinned. This used to be `corepack enable`, which resolves whatever each
# checkout's own packageManager field pins: a second source of truth for a
# version the base now owns, and one that made agent-shell's pnpm differ from
# the one remote-chat-agent runs.

# mise -- baked into the image itself (system path, MISE_INSTALL_PATH),
# unlike the tools IT manages (which persist per-container under
# MISE_DATA_DIR=/workspace/tools/mise on the shared volume). The `mise`
# manager `wf vm deps replay`/`wf vm deps install mise
# <tool>` already shell out to (clis/deploy/src/agent_deps.rs)
# assumes a bare `mise` on PATH and does not bootstrap it -- without this,
# every `mise install`/`mise which` call in that flow fails with `spawn
# mise ENOENT`. A real Dockerfile RUN step,
# not a runtime install under the persistent HOME, so it survives even a
# full /workspace volume loss and needs no PATH surgery in entrypoint.sh's
# /etc/environment reconstruction.
RUN curl -fsSL https://mise.run | MISE_INSTALL_PATH=/usr/local/bin/mise sh

# Docker Engine (dockerd + CLI + containerd + runc + buildx + compose),
# from Docker's own official apt repo -- NOT Debian bookworm's plain
# `docker.io` package, which was the first thing tried here. Confirmed
# live: `docker.io` has no `buildx` CLI plugin at all (Debian doesn't
# package it), so scripts/build-and-push-image.sh failed immediately on
# `docker buildx build` with "unknown shorthand flag: 'f' in -f" -- the CLI's own
# fallback parse of an unrecognized `buildx` subcommand, not a real flag
# error. This is the exact same repo (download.docker.com) and package
# set (docker-ce, docker-ce-cli, containerd.io, docker-buildx-plugin,
# docker-compose-plugin) this repo's own Claude Code Remote sandbox
# environment already uses (confirmed via `dpkg -l` there) --
# reproducing that, not inventing a new source. `iptables`/`uidmap` are
# dockerd's own runtime deps for its default bridge networking and
# rootless-capable setup.
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates gnupg \
    && install -m 0755 -d /etc/apt/keyrings \
    && curl -fsSL https://download.docker.com/linux/debian/gpg -o /etc/apt/keyrings/docker.asc \
    && chmod a+r /etc/apt/keyrings/docker.asc \
    && echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/debian bookworm stable" \
       > /etc/apt/sources.list.d/docker.list \
    && apt-get update && apt-get install -y --no-install-recommends \
      docker-ce \
      docker-ce-cli \
      containerd.io \
      docker-buildx-plugin \
      docker-compose-plugin \
      iptables \
      uidmap \
      fuse-overlayfs \
    && rm -rf /var/lib/apt/lists/*

# Playwright + a real Chromium, installed the same way this repo's own
# Claude Code Remote sandbox sessions get theirs (this repo's CLAUDE.md:
# `PLAYWRIGHT_BROWSERS_PATH=/opt/pw-browsers`) -- a fixed, known path rather than each user's default
# cache dir, so it survives being invoked from any shell/user context.
# `--with-deps` pulls in the system libs (libnss3, libatk, etc.) Chromium
# needs to actually launch headless, not just unpack.
#
# NODE_PATH: `npm install -g` only puts a package's CLI bin on $PATH, not
# on Node's module resolution path -- confirmed live, `require('playwright')`
# from an arbitrary script (not one npm itself invokes) fails
# MODULE_NOT_FOUND without this. `npm root -g` is fixed for this image
# (baked at build time, not re-queried per shell), so a plain ENV is
# enough; every `node`/`import`/`require` in any interactive session picks
# it up automatically since it's a real environment variable, not a
# per-invocation flag someone has to remember.
ENV PLAYWRIGHT_BROWSERS_PATH=/opt/pw-browsers
RUN --mount=type=secret,id=extra_ca_cert,required=false --mount=type=cache,target=/root/.npm \
  NODE_EXTRA_CA_CERTS=/run/secrets/extra_ca_cert npm install -g playwright \
  && npx playwright install --with-deps chromium
ENV NODE_PATH=/usr/local/lib/node_modules
ENV WF_SITE=vm

# Interactive coding CLIs -- same install line cloud-init.sh.tftpl uses for
# the host itself, so behavior matches whichever surface a human ends up on.
#
# --mount=type=secret ... required=false: opt-in extra CA trust for a
# TLS-intercepting build-time network (a corporate/sandboxed CI egress
# proxy) -- inert by default, same pattern as
# clis/vnc-auth-capture/Dockerfile (see that file's own comment for the
# live verification this doesn't affect a build that hasn't opted in).
RUN --mount=type=secret,id=extra_ca_cert,required=false --mount=type=cache,target=/root/.npm \
  NODE_EXTRA_CA_CERTS=/run/secrets/extra_ca_cert npm install -g @anthropic-ai/claude-code @openai/codex opencode-ai

# Google Cloud CLI. Not in Ubuntu's archive, so it needs Google's own apt
# repo -- which is why `wf vm deps install apt google-cloud-cli` cannot
# provision it: that path runs `apt-get install` alone and dies with
# "Unable to locate package google-cloud-cli". Baked here so a redeploy
# keeps it, matching clis/vnc-auth-capture/scripts/setup.sh's install_gcloud.
# `gcloud auth login --enable-gdrive-access` is what makes this credential a
# general Google account (Drive/Docs) rather than only a gcloud one.
RUN curl -fsSL https://packages.cloud.google.com/apt/doc/apt-key.gpg \
      | gpg --dearmor -o /usr/share/keyrings/cloud.google.gpg \
  && echo "deb [signed-by=/usr/share/keyrings/cloud.google.gpg] https://packages.cloud.google.com/apt cloud-sdk main" \
      > /etc/apt/sources.list.d/google-cloud-sdk.list \
  && apt-get update \
  && apt-get install -y --no-install-recommends google-cloud-cli \
  && rm -rf /var/lib/apt/lists/*

# git, curl, ca-certificates and unzip now come from the base, as do gh and bws;
# only what is genuinely agent-shell's own remains. python3/make/g++ are
# node-gyp's build deps for the native modules this monorepo's pnpm install
# pulls in (bufferutil, utf-8-validate).
#
# expect/python3-pexpect used to live here too, backing e2e-wf-smoke.sh's
# real-login-shell checks (interactive-checks/check.exp + check.py, run via
# `su - agent -l`). They moved out of the image into an ssh-driven e2e test
# (since P6 a `bash -lc` check in e2e-wf-smoke.sh) -- once real `ssh`
# (over the tailnet, via a narrow machine-only test identity) was confirmed
# working end to end, there was no reason to keep approximating a login
# shell from inside this container when the real thing is directly
# reachable from outside it. Those checks no longer use expect/pexpect at
# all: the env/noise ones run as a plain one-shot `ssh host "bash -lc ..."`,
# and the terminal-UX ones through tmux + ssh driven from
# the CLIENT side, so neither driver is needed in this image either.
# See docs/archive/runbooks/agent-shell-wf-harness-interactive.md.
# The tool packages most often edited (ssh, tmux, build deps) sit here, after the heavy installs above, so adding one reruns
# only this layer and what follows instead of the whole chain.
RUN apt-get update && apt-get install -y --no-install-recommends \
      openssh-server \
      tmux \
      python3 \
      make \
      g++ \
      rsync \
      less \
      file \
      restic \
      unzip \
      curl \
      ca-certificates \
      sqlite3 \
    && rm -rf /var/lib/apt/lists/*

# Ubuntu's rclone is built without the oracleobjectstorage backend the state backup needs, so rclone is the pinned upstream binary.
RUN curl -fsSL -o /tmp/rclone.zip https://downloads.rclone.org/v1.68.2/rclone-v1.68.2-linux-arm64.zip \
    && echo "c6e9d4cf9c88b279f6ad80cd5675daebc068e404890fa7e191412c1bc7a4ac5f  /tmp/rclone.zip" | sha256sum -c - \
    && unzip -p /tmp/rclone.zip '*/rclone' > /usr/local/bin/rclone && chmod 0755 /usr/local/bin/rclone \
    && rm -f /tmp/rclone.zip

# The OCI CLI, for `wf cache guard --local`: the CI runner asks this container, which holds the OCI secrets, to measure and evict
# Object Storage through vm-ops-api's exec route, so no OCI key ever sits on the runner. ~/.oci/config is written at first use from the
# vault by the guard itself (the same OCI_* secrets `wf vm creds sync` reads). A venv keeps its dependencies off the system python;
# the version is pinned (the guard deletes with the admin key), so a bump is a reviewed change.
RUN apt-get update && apt-get install -y --no-install-recommends python3-venv \
    && python3 -m venv /opt/oci-cli \
    && /opt/oci-cli/bin/pip install --no-cache-dir oci-cli==3.95.1 \
    && ln -s /opt/oci-cli/bin/oci /usr/local/bin/oci \
    && rm -rf /var/lib/apt/lists/*

# wf CLI: the VM's one build on the `clis` volume (clis.yml), mounted at /opt/clis.
RUN ln -sf /opt/clis/current/wf /usr/local/bin/wf

# Small wrapper around vm-ops-api's own /services/:app/redeploy -- the
# scoped, no-docker-socket way this container redeploys the VM's OTHER
# backend apps. See README.md in this directory for the security reasoning.
COPY infra/oracle-cloud/agent-shell/redeploy-app.sh /usr/local/bin/redeploy-app
RUN chmod 755 /usr/local/bin/redeploy-app

# 15-min BetterStack heartbeat (see heartbeat.sh) -- launched detached by
# entrypoint.sh before it execs into sshd.
COPY infra/oracle-cloud/agent-shell/heartbeat.sh /usr/local/bin/agent-shell-heartbeat
RUN chmod 755 /usr/local/bin/agent-shell-heartbeat

# T3 Code server supervisor (see t3code-serve.sh) -- launched detached by
# entrypoint.sh, guarded by AGENT_SHELL_T3CODE_ENABLED, same as the
# heartbeat above. Off by default until verified end to end.
COPY infra/oracle-cloud/agent-shell/t3code-serve.sh /usr/local/bin/t3code-serve
RUN chmod 755 /usr/local/bin/t3code-serve

# Login tracking: sourced by /etc/profile for every interactive login shell
# (see the file's own comment for why this container needs its own
# mechanism rather than relying on wtmp/lastlog).
COPY infra/oracle-cloud/agent-shell/track-login.sh /etc/profile.d/99-track-login.sh
RUN chmod 644 /etc/profile.d/99-track-login.sh

# Command-history flush: appended to /etc/bash.bashrc (sourced by every
# interactive bash shell, login or not -- unlike /etc/profile.d, which only
# fires for login shells) so a command is captured in ~/.bash_history
# immediately after it runs, not only once the shell eventually exits.
# Without this, a heartbeat firing mid-session would see none of that
# session's commands yet. HISTTIMEFORMAT timestamps each entry so the
# heartbeat's tail is actually useful, not just a bare command list.
RUN printf '\n# BetterStack heartbeat support (infra/oracle-cloud/agent-shell) -- flush\n# history immediately and timestamp it, so ~/.bash_history reflects an\n# in-progress session, not only closed ones.\nexport HISTTIMEFORMAT="%%F %%T "\nexport PROMPT_COMMAND="history -a${PROMPT_COMMAND:+; $PROMPT_COMMAND}"\n' >> /etc/bash.bashrc

# Waits for the nested dockerd to be ready, then `docker login`s as the
# `agent` user (so the resulting ~/.docker/config.json lands in agent's own
# HOME, not root's) -- see the script's own comment for the no-op-when-
# unset behavior.
COPY infra/oracle-cloud/agent-shell/docker-login.sh /usr/local/bin/docker-login.sh
RUN chmod 755 /usr/local/bin/docker-login.sh

# checks that the baked wf runs (see the script's header comment)
COPY infra/oracle-cloud/agent-shell/e2e-wf-smoke.sh /usr/local/bin/e2e-wf-smoke
RUN chmod 755 /usr/local/bin/e2e-wf-smoke

# Dedicated non-root login user. Deliberately never root and never `sudo`
# -- but IS in the `docker` group now, unlike before. That's a real
# capability change: `agent` can talk to the dockerd running inside THIS
# container without sudo, the same way the host's `ubuntu` account can
# talk to the HOST's dockerd. The difference that matters is which daemon
# is on the other end of that group membership -- this one is nested
# inside agent-shell's own container with no visibility into the host's
# Docker Engine, the other apps' containers, or the host filesystem, so
# the well-known "docker group = root on this machine" escalation path
# only reaches this container's own throwaway nested environment.
RUN groupadd -f docker \
    && useradd -m -s /bin/bash -G docker agent \
    && mkdir -p /home/agent/.ssh \
    && chmod 700 /home/agent/.ssh \
    && chown -R agent:agent /home/agent

# Must come after `useradd` above -- `chown agent:agent` needs the account
# to already exist. Owned by `agent` (not root) since track-login.sh writes
# here from inside that user's own login shell.
RUN mkdir -p /var/log/agent-shell && chown agent:agent /var/log/agent-shell

# Minimal, explicit sshd config: key-only auth, no root login, no password
# fallback -- narrower than Debian's own defaults, not relying on them.
COPY infra/oracle-cloud/agent-shell/sshd_config /etc/ssh/sshd_config

COPY infra/oracle-cloud/agent-shell/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

RUN mkdir -p /run/sshd
EXPOSE 22
ENTRYPOINT ["/entrypoint.sh"]
