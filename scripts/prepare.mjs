#!/usr/bin/env node
// Installs this repo's git hooks (lefthook) for local dev — meaningless (and
// broken: no `git` binary) inside a Docker image build, which never needs a
// git working tree or its own hooks. See scripts/postinstall.mjs for the
// same DOCKER_BUILD=true gating pattern and why it's set by each backend
// Dockerfile.
import { execSync } from 'node:child_process';

if (process.env.DOCKER_BUILD !== 'true') {
  execSync('lefthook install --force', { stdio: 'inherit' });
}
