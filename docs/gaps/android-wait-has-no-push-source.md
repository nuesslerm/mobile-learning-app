---
status: open
area: device-lab
paths:
  - "clis/harness-android/src/driver.rs"
opened: 2026-10-12
last_verified: 2026-10-20
links: []
---
# Android waits sample because the emulator offers no change feed

## What is broken

`wait` and `wait_gone` on Android poll the UI dump; the browser driver subscribes (MutationObserver). Budget and slow-vs-broken reporting match, but a wait costs a dump per sample.

## Why blocked

Not blocked.

## What unblocks it

Subscribe to accessibility events (`uiautomator events` or an instrumentation service) and wake waits from them.

## Re-checked 2026-10-20

Re-read the Android wait path on dev 95e494827. `wait` now makes one on-device uiautomator2 `waitForExists` round trip (android_u2.rs:230-254). `wait_gone` and `wait_editor_longer` still use the sampling defaults in device/mod.rs:60-103, at 100 ms per sample. Nothing in clis/wf*/src subscribes to accessibility events, so Android still has no change feed. The cost per sample is now an RPC, not a dump (CI run-36ad2767cb8c74f3: wait_gone 1.9 s for one call).

Out of date above:

- Harness API H6 (20 Oct) dropped the Android CLI's old `wait` verb and with it the one `waitForExists` call: every Android wait now samples `objInfoOfAllInstances` every 100 ms (`android_input::wait_visible`), the dump only under WF_ANDROID_UIA=dump.
- "a wait costs a dump per sample": since 69e965551 (12 Oct) and 3fdb17600/4d4ec2383 (16 Oct), waits go through the in-process uiautomator2 server. `wait` is one on-device waitForExists call (android_u2.rs:230-254), and `wait_gone`/`wait_editor_longer` sample an objInfo RPC or an edit-text child every 100 ms (device/mod.rs:60-103). A uiautomator dump happens only as the fallback (WF_ANDROID_UIA=dump or an unusable server, android_input.rs:115-123, 201-218).
- "Budget and slow-vs-broken reporting match": true for `wait` only (android.rs:209-231, browser.rs:~320-335). Neither `wait_gone` reports slow or broken, and the Android one (device/mod.rs:98) leaves out the screen state the browser's includes (browser.rs:355).
- frontmatter `paths` lists only clis/harness-android/src/driver.rs. The sampling now lives in clis/harness-core/src/device/mod.rs (the trait defaults), clis/harness-android/src/android_u2.rs and clis/harness-android/src/android_input.rs, so add those.
- "What unblocks it" names `uiautomator events` or an instrumentation service. The UI server already holds a live UiAutomation connection (android_u2.rs:1-4), which is the natural place to wire accessibility events now.
