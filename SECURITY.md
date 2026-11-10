# Security policy

This repository is a public copy of a private project. No secret is stored in git: the credentials CI needs are GitHub Actions secrets of this repository (the two vault access tokens and the tailnet-join client).

The CI runs one pipeline (`pipeline.yml`) in two ways that behave identically: on a GitHub-hosted runner (`ci-hosted.yml`) and on a self-hosted runner on the owner's own machine (`deploy.yml`). Both reach the owner's private network. The sections below say how that is kept safe.

Report a vulnerability through GitHub's private vulnerability reporting (the Security tab, "Report a vulnerability"), not through a public issue. If you find a credential, key or personal detail in the tree, report it the same way.

## How CI is kept safe

- Every workflow starts only from a push to `dev`, a manual dispatch, a schedule or another workflow. None starts from a pull request, so a fork cannot run code with the repository's secrets or on its self-hosted runner.
- Only the owner can create, push to or delete a branch (a repository ruleset); `dev` is the CI branch and takes no merges from others. Nobody else has write access.
- The secrets are GitHub Actions secrets, never in git, and the workflows mask every value they fetch.
