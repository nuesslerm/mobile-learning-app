---
title: EAS publishing and dev launch args
date: 2026-08-30
category: workflow
tags: [eas, expo, publish]
problem_type: knowledge
module: frontend/mla-tamagui
---

# EAS Publishing & dev launch args

## Publishing an EAS Update

```bash
pnpm --filter mla-tamagui run publish:eas -- --initial-route /your-route
pnpm --filter mla-tamagui run publish:eas -- -r /your-route   # short form
```

`--initial-route` is mandatory — the publish fails without it. The route is inlined
as `EXPO_PUBLIC_INITIAL_ROUTE` so the QR code cold-starts on the right screen.

Optional overrides:
```bash
pnpm --filter mla-tamagui run publish:eas -- -r /route --branch <name> --message "msg"
pnpm --filter mla-tamagui run publish:eas -- -r /route --platform all   # iOS + Android (default: ios)
EAS_UPDATE_PLATFORM=android pnpm --filter mla-tamagui run publish:eas -- -r /route
```

Runtime version: `exposdk:57.0.0` — opens directly in Expo Go.

## Dev server route

For a local dev server, run this in `frontend/mla-tamagui` (an `initial_route` set in
`dev-runtime-config.json` still wins outside production):

```bash
EXPO_PUBLIC_INITIAL_ROUTE=/leetcode/two-sum npx expo start
```

For a shareable preview that opens on a route, publish an EAS Update:

```bash
wf ship eas --initial-route /leetcode/two-sum
```

`app/+native-intent.ts` opens the baked route on a cold start that names no screen (a deep link wins).

No shared dev Metro runs: an update bakes the route and the `EXPO_PUBLIC_*` variables of
the environment `wf ship eas` runs in, so a branch preview can carry its own route and
`EXPO_PUBLIC_APP_ENV`. A hand publish lacks what CI's `eas-update` job adds (the backend
hosts and token, the lab flag, the BetterStack pair): good for UI, not for Run Local or
the coding agent.

## Dev runtime config (without re-bundling)

Mutate `frontend/libraries/app-config/dev-runtime-config.json` by hand (no package
script edits it any more): set `"initial_route": "/leetcode/two-sum"`, add a flag
under `feature_flags` (`"shiny": true`), and set `initial_route` back to `null` to
reset.

Inline at launch (canary path, no file mutation), once `expo-native-launch-arguments`
is installed:
```bash
# Maestro
launchApp { arguments: { devConfigJson: '{"initial_route":"/leetcode/two-sum","feature_flags":{}}' } }
```

Today this does nothing. `frontend/libraries/app-config/src/nativeLaunchArgs.ts`
requires `expo-native-launch-arguments`, which no `package.json` declares, so the
require throws and every launch arg reads as `{}` (always so in Expo Go). Flags
passed to `npx expo start` would not set native launch args anyway.

## Supported launch args

| Arg | Effect |
|---|---|
| `devConfigJson` | Inline DevRuntimeConfig JSON for the session. No effect until `expo-native-launch-arguments` is installed. |
| `EXDevMenuIsOnboardingFinished` | Skip Expo Dev Menu onboarding overlay. **Required for all Maestro flows.** |

## Maestro pre-warm

Every Maestro flow that cold-launches the app must include:
```yaml
- launchApp:
    stopApp: true
    arguments:
      EXDevMenuIsOnboardingFinished: true
```
Without this, the dev menu onboarding overlay intercepts the first `tapOn`.

## `eas.json` cannot carry comments (mla-tamagui's `ios.scheme`)

`eas.json` is schema-validated with no additional properties, at every level of a
build profile. A `"//"` key — the usual JSON-comment idiom — is rejected outright:

```
eas.json is not valid.
- "build.simulator-dev.ios.//" is not allowed
```

and moving it up a level only changes which path it names
(`- "build.simulator-dev.//" is not allowed`). This is not specific to `build`:
`eas update` validates the whole file too, so a stray comment key blocks
**publishing**, not just building. So the note that used to live inside
`frontend/mla-tamagui/eas.json` lives here instead:

**`ios.scheme` in a build profile is the Xcode scheme, NOT `app.config.js`'s
`expo.scheme`.** `expo prebuild` names the Xcode project after `expo.name` with
its own sanitisation (`mla-tamagui` → `mlatamagui`), while `expo.scheme` is the
deep-link URL scheme (`mla-tamagui`) that every `exp://` link in this repo
depends on. Without `"scheme": "mlatamagui"`, EAS's fastlane step looks for a
scheme named after the deep-link one and fails with `scheme 'mla-tamagui' does
not exist` — the same pre-existing bug `docs/gaps/no-real-device-verification.md`
records against app #1 (in full in `docs/archive/README.md`). Naming the Xcode scheme in `eas.json` fixes the build without touching
the deep-link scheme.