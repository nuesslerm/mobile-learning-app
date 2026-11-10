---
status: open
area: remote-chat-agent
paths:
  - "backend/remote-chat-agent/**/docker-compose*.yml"
  - "backend/remote-chat-agent/opencode-server/**"
opened: 2026-10-04
last_verified: 2026-10-20
links: []
---
# The published `opencode-server` image is arm64-only

## What is broken

The multi-arch build is committed (df1ef5b) but `docker manifest inspect` shows the published image
arm64 only, so the remote-chat-agent e2e compose cannot start in an amd64 sandbox.

## Why blocked

Images are built natively on the arm64 VM; nothing publishes the amd64 variant.

## What unblocks it

Build and push a multi-arch manifest (or an amd64 tag for e2e use), or run that e2e suite on arm64 only
and say so in its runbook.

## Re-checked 2026-10-20

`docker manifest inspect` and the Docker Hub tags API both show only arm64 for opencode-server, including `latest` and `sha-95e494827`, which CI run 39395319187 pushed today. The cause is `clis/deploy/src/build_image.rs:128`: every VM build passes `--platform linux/arm64`, and no committed path builds an amd64 variant. df1ef5b, the multi-arch commit this record cites, is not in the dev history, and the `build-images.yml` it edited no longer exists. CI is not affected, because its deployment verification runs `test:verify:deployed` against the deployed service. A local `test:e2e` compose run in an x86_64 sandbox still cannot use the image, and sandbox-first CI (`docs/plans/sandbox-first-ci/spec.md:56`, which describes the sandbox as linux-x64) would run into the same problem.

Out of date above:

- "The multi-arch build is committed (df1ef5b)": not true on dev. df1ef5b (2026-08-13, which edited `.github/workflows/build-images.yml`) is not an ancestor of dev (only `origin/claude/golden-screenshot-tests` has it), and `build-images.yml` no longer exists. Images are built by wf at `clis/deploy/src/build_image.rs:128` with `docker buildx build --platform linux/arm64`, and no multi-arch build is committed.
- "Images are built natively on the arm64 VM": still true, but name the file: `clis/deploy/src/build_image.rs:128` (the `--platform linux/arm64` flag) and its `tag_latest` helper at `:198`.
- frontmatter `last_verified: 2026-10-04` should be 2026-10-20.
