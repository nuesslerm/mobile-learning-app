---
status: blocked
area: mobile-app
paths:
  - "frontend/mla-tamagui/app/**"
  - "frontend/mla-tamagui/app.config.*"
  - "frontend/mla-tamagui/eas.json"
  - "frontend/mla-tamagui/features/tamil-writing/**"
  - "frontend/libraries/tamagui-design-system/composites/**"
opened: 2026-08-16
last_verified: 2026-10-04
links:
  - solutions/workflow/eas-publishing-checklist.md
  - solutions/testing/verification-ladder.md
  - solutions/ui-bugs/overlays-that-are-green-in-a-browser-and-invisible-on-a-dark-device.md
---
# Nothing in the app has been verified on a real device or simulator from a session

## What is broken

Published EAS Updates of `frontend/mla-tamagui` are loadable in Expo Go, but every check is web or
flow-test only. Open items that need eyes on a device:
- Black screen on a real iPhone after a clean JS boot (`useFonts` error-hang fix shipped, never re-tried).
- Liquid Glass (`expo-glass-effect`, iOS 26 only) drawing correctly in the `glass` Material tier.
- Tamil tracing pad gesture and glyphs (`frontend/mla-tamagui/e2e/verify-tamil.mjs` is Playwright on web).
- Voice-edit `AbortController` timeout fix (replaced `AbortSignal.timeout`, missing on Hermes).
- Whether a newly published EAS Update reaches a phone that already has the app open, without reopening it.
Server-side logs stop at JS; a native crash or blank render after JS boot is invisible server-side.

## Why blocked

Sessions run on Linux: no `xcrun`/`simctl`/`mobilecli`, no `/dev/kvm`. `eas simulator:start` answers
"EAS Simulator isn't available on nuesslerm yet" (waitlist, 2026-08-29), bills when it is, and boots
without Expo Go. A dev-client simulator build hit an Xcode scheme mismatch
(`scheme 'expohelloworldapp2' does not exist`) on the old app; unconfirmed for mla-tamagui.

## What unblocks it

One human opens the published update (the `### EAS Update` link of a CI run summary, or
`wf ship eas --initial-route <route>`) on a phone and walks the items above; or EAS Simulator reaches the account
and a dev-client simulator build exists. Report results back into this gap, then close it.
