# iOS harness implementation

## Goal

Implement a shared Lua iOS Platform and Device in `clis/wf-harness/src/ios.rs`, wired through `lib.rs` exporting `wf_harness::ios::run`. Provide the full `Device` trait (harness-core) and `Platform` trait (harness-core) for iOS simulator via idb.

## Constraints

- **Explicit idb UDID**: The driver must accept a `--device` UDID (or default to the only booted simulator) and pass it to every idb invocation.
- **AX lookup viewport clipping**: Element bounds must be clipped to the simulator's screen (the device frame). The driver must read accessibility trees via `idb ui describe-all --json` and compute visible bounds.
- **Swipes**: Implement `swipe` using `idb ui swipe` (or `idb ui drag`).
- **HID typing and key**: Implement `input_text` via `idb ui text` (or `idb ui key` for key presses). Implement `press_key` for iOS keyboard keys (Enter, Backspace, Tab, arrow keys, Ctrl+End).
- **Keyboard state**: Implement `keyboard_visible` via `idb ui describe-all` and detect the keyboard view.
- **Video SIGINT**: Implement `start_recording` and `stop_recording` via `idb video record`; stop must handle SIGINT (the recording ends cleanly when the process is interrupted).
- **Screenshot**: Implement `take_screenshot` via `idb screenshot -` (PNG).
- **Continuous Tamil glyph gesture**: Implement `draw_strokes_on` for Tamil glyphs, using `idb ui scribble` or equivalent stroke input.
- **No app reset or device use**: The driver must not reset the app (no `idb launch` or `idb kill`), and must not start/stop the simulator.
- **No pushes**: The driver must not push files or install apps.

## Verification

- `cargo check` using `CARGO_TARGET_DIR=/Users/markus/conductor/workspaces/mobile-learning-app/zagreb-v1/clis/target`
- The harness must integrate with existing `wf test` flow tests; an iOS flow test must run and pass (no-op initially).
- The `wf ios` CLI must appear in `wf --help` and accept `test` subcommand.

## Done when

- `wf ios test <script>` runs a Lua script on the iOS simulator.
- All Device trait methods are implemented (may stub unimplemented ones with `unimplemented!()` initially, but must compile).
- The iOS driver passes a minimal smoke test (open simulator, tap, screenshot) without crashing.
- The change is merged to `dev` and an EAS Update is published (if UI changes).