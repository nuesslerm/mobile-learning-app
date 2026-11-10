---
paths: frontend/**
name: expo-web-validation
description: Browser validation workflow for Expo Router apps that must behave in both Expo Go and Expo web/GitHub Pages. Use when testing design-system screens, Storybook stories, NativeWind/RN Reusables primitives, keyboard/WebView/sheet interactions, or when using Playwright to prove a web export is hydrated and interactive rather than screenshot-like.
---

# Expo Web Validation

Use this skill to validate Expo Go-safe React Native UI in the browser.

## Setup

1. Read the repo's `AGENTS.md` first. If it requires versioned Expo docs, open the exact SDK docs before changing code.
2. Install dependencies without lifecycle scripts when this repo's `postinstall-postinstall` hook fails:
   ```bash
   npm install --ignore-scripts
   ```
3. Install the Chromium browser used by Playwright:
   ```bash
   npx playwright install chromium
   npx playwright install-deps chromium
   ```
4. Prefer the repo script for repeatable validation:
   ```bash
   npm run test:web:playwright:export
   ```
   Only pass `--base-url <live-url>` from an environment with ordinary, unrestricted network access (a developer's machine, CI) — never from inside a sandboxed Claude Code agent session. See step 4 below and `docs/solutions/mcp/chromium-post-quantum-tls-breaks-sandbox-proxy.md`.

## Playwright workflow

1. Validate hydration, not just pixels:
   - Visit the route under review.
   - Assert `#root` contains expected app text.
   - Assert unsupported placeholders are absent, especially `React Native WebView does not support this platform.`
2. Exercise interactions with stable selectors:
   - Prefer `page.getByTestId(...)` for app controls.
   - For Storybook's side nav, use a visible-text helper because the tree may contain hidden duplicate labels.
   - Click tabs, buttons, toggles, inputs, sheets, and any WebView-backed editor in the story under review.
3. Capture evidence:
   - Save screenshots to `artifacts/web-validation/`.
   - Record counts for critical DOM bridges such as iframe-backed WebViews.
   - Treat page errors and non-BetterStack request failures as failures.
4. Validate the local export, then deploy, when changing web behavior:
   ```bash
   EXPO_PUBLIC_INITIAL_ROUTE=/design-system/storybook npx expo export --platform web --output-dir dist-gh-pages --clear
   npm run test:web:playwright:export
   npm run deploy:web:pages -- --initial-route /design-system/storybook
   ```
   **Do not add a live-URL Playwright check (`--base-url https://nuesslerm.github.io/...`) from inside a sandboxed Claude Code agent session.** Headless Chromium there cannot complete a TLS handshake with any external HTTPS host through the sandbox's mandated egress proxy — Chrome's default post-quantum ClientHello (`X25519MLKEM768`) is larger than a single TCP segment, which the proxy's TLS-termination layer resets rather than reassembles, and there's no Chrome flag left to disable it (removed as of Chrome ≥138). `curl`/`git` have no such extension and go through fine, which is why this looks host-specific but isn't. `deploy-gh-pages.sh` already runs the local-export check above as its own pre-push validation — that's the in-sandbox bar. A live-URL smoke test is still reasonable, just from a machine with normal network access. Full diagnosis: `docs/solutions/mcp/chromium-post-quantum-tls-breaks-sandbox-proxy.md`.

## Repo-specific checks

For design-system work in this repo, the minimal browser matrix is:

- `/design-system` — app-shell hydration and basic buttons/toasts.
- `/design-system/storybook` → `Showcases/Web Parity/Interaction Matrix` — NativeWind + RN Reusables primitives.
- `/design-system/storybook` → `Composites/KeyboardToolbar/Text Areas` — target-scoped toolbar, avoidance, and no-drift coverage. (There is no `WebView` story in this file anymore — only `Text Areas` is exported.)

For local static exports, use `npm run test:web:playwright:export`; it starts the repo's built-in static server with the same `/expo-hello-world-app-2` base path that GitHub Pages uses, so Playwright does not depend on ad-hoc `serve` mounting behavior.

After committing and pushing, publish the required EAS Update with the screen under review:

```bash
npm run publish:eas -- --initial-route /design-system/storybook
```
