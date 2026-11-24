# CodeMirror timing in Buoy

Run Metro with `EXPO_PUBLIC_EDITOR_PERF=1` and open the Expo app on a simulator.
The development build's existing Buoy Console tool receives editor timings.
In MCP, call `get_console` with `pattern: "editor.input_timing"`, or filter
for the same event in Desktop. Use the same device, build mode, document,
language, and keyboard for both sides of a comparison.

- `editor.cold_start` reports editor mount, WebView load, page readiness, and
  hydration intervals. `nth` identifies the editor instance in this JS launch.
- `editor.input_timing` samples document changes at most four times per
  second. `inputToUpdateMs` measures from the WebView's `beforeinput` to the
  CodeMirror update and is null for edits without that event. `updateToFrameMs`
  measures the wait to the next WebView animation-frame callback. That callback
  precedes paint. `frameToHostMs` measures delivery to React Native using the
  device wall clock. Disable clock overrides during capture.

To investigate a blank editor, open a problem, navigate away and back, and
check for a new `nth` and its cold-start timings. To investigate typing, wait
for hydration, then enter the same text at the same position. The probe only
sends timings and counts. The dev-only Buoy transport forwards those events
through the existing console capture, while the editor calls the standard
logger. Buoy's JS-thread profiler samples React Native. The editor probe
samples the separate WebView.

## Tracing a generated stack to source

EAS can store release source maps alongside the update. Use
`wf android stack <update-group> --platform ios --file <stack-file>` to map
a captured Hermes stack using the iOS update's map. The command name reflects
where the harness first implemented symbolication. See the
[call-site contract](../solutions/conventions/logger-call-site-contract.md)
for the fields Babel currently adds to routine log events. Removing that
plugin requires a migration for lexical function names and test IDs in
addition to symbolication for events that carry stacks. The HTML-contained
CodeMirror bundle has its own generated code and needs its own map for
WebView stack traces.
