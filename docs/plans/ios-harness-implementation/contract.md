# Contract: iOS harness

## Public shapes

**File**: `clis/wf-harness/src/ios.rs`

**Module**: `wf_harness::ios`

**Exports**:
- `pub fn run(args: &[String]) -> Option<i32>`: CLI handler for `wf ios`.
- `pub struct IosPlatform`: implements `harness_core::platform::Platform`.
- `pub struct IosDevice`: implements `harness_core::device::Device`.

**Types**:
- Use existing `harness_core::device::{Device, Element, Key, Part, Rect, Sel}`.
- `IosDevice` holds a `String` UDID (or `None` for default).

**CLI integration**:
- `wf_harness::COMMANDS` extended with `("ios", ios::run)`.
- `wf_harness::HELP` extended with iOS help table (can be empty initially).

## Decisions

1. **UDID handling**: `--device` argument passes UDID; if omitted, driver uses the first booted simulator (via `idb list-targets`). The UDID is stored as `String`.
2. **AX viewport clipping**: The simulator's screen size is read via `idb ui describe-all` root node bounds. Element visible bounds are intersection of element bounds with screen bounds.
3. **Keyboard detection**: The keyboard is considered visible if the accessibility tree contains a node with `AXIdentifier` containing `"Keyboard"`. `keyboard_visible` returns that bool.
4. **Video recording**: `start_recording` runs `idb video record --udid <UDID> <path>`. `stop_recording` sends SIGINT to the recording process (by pid) and waits for file.
5. **Continuous Tamil glyph gesture**: `draw_strokes_on` uses `idb ui scribble` with stroke points derived from the glyph source data. The source is a string of stroke commands (e.g., `"M x,y L x,y ..."`). The driver transforms coordinates to element bounds.
6. **Unimplemented methods**: For first version, stub `add_network_stub`, `get_network_stub_hits`, `get_debug_hits`, `arm_logpoints`, `stub_routing` with `unimplemented!()` (panic). These are not needed for core flows.
7. **Error handling**: All idb calls wrap `Command` output; errors produce `Err(String)` with descriptive message.

## Verification command

From repo root:
```bash
CARGO_TARGET_DIR=/Users/markus/conductor/workspaces/mobile-learning-app/zagreb-v1/clis/target cargo check -p wf-harness --features testing
```

A flow test will be added later (separate unit).