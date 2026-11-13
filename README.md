# mobile-learning-app
> This is a public copy of a private project, exported without history. It holds no secret in git: the credentials CI needs are GitHub Actions secrets of this repository, and the one CI pipeline runs on GitHub-hosted runners only, only from pushes to the protected `dev` branch, never from pull requests (see `SECURITY.md`).

Monorepo for the mla-tamagui mobile learning app and the tooling, backend services, and infrastructure around it.

## What lives here

- [`frontend/mla-tamagui/`](frontend/mla-tamagui/) — Expo Router app.
- [`frontend/mla-tamagui/features/`](frontend/mla-tamagui/features/) — one directory per feature, each owning its vertical slice.
- [`clis/`](clis/) — repo tooling surfaced through the `wf` CLI.
- [`libraries/`](libraries/) — shared packages and design-system primitives.
- [`backend/`](backend/) and [`infra/`](infra/) — deployable services and cloud infrastructure.
- [`docs/`](docs/) — architecture notes, runbooks, and documented solutions.
- [`AGENTS.md`](AGENTS.md) — agent workflow rules and repo conventions.

## Quickstart

Use the tool versions pinned in [`mise.toml`](mise.toml) and the `packageManager` entry in [`package.json`](package.json).

```bash
pnpm install
pnpm --filter mla-tamagui start   # a local Metro for your branch
```

Open the app from Metro with Expo Go, or target a platform directly:

```bash
wf android start-device      # the redroid device (CI runs it), then: wf android open-link <exp:// url>
```

## Common checks and deploys

```bash
wf check --staged                              # the pre-commit gate: lint, affected typecheck and tests, dead references
pnpm --filter mla-tamagui run test:flow        # the app's flow tests
wf browser test e2e/editor-typing.lua          # one browser e2e test, recorded, with its report
wf ship eas --initial-route /your-route        # publish an EAS Update: open its link in Expo Go on a phone (CI's run summary links the pushed commit's)
```

`wf ship eas` reads `EXPO_TOKEN` from the vault.

## More context

- [Expo SDK 57 docs](https://docs.expo.dev/versions/v57.0.0/)
- [Shared domain vocabulary](CONCEPTS.md)
