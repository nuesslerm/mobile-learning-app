# @lib/flow-test-harness

Flow tests perform faithful interactions with real UI elements and check that
business logic runs. The harness is one opinionated, NestJS-inspired extension
of Testing Library: a whole screen renders the way the web export renders it,
the test interacts with it by testID through a session, and the component's own
handlers and business logic run. Tests never build ad hoc helpers.

## Rules

- **No `vi.mock`.** Screens render through the real react-native-web / Tamagui
  stack inside jsdom; the per-package mocks flow tests used to duplicate are
  gone.
- **No component stubs.** The tree is the real tree: real DOM, real testIDs,
  real text. Only a testID addresses an element.
- **Ports for business logic.** Anything a test must fake or seed — navigation,
  persistence, audio recording, the code editor bridge — is injected through a
  harness port (`routerHarness`, `storageHarness`, dependency `overrides`), not
  patched in the test.
- **Harness stubs only for native runtimes.** A `.cjs` stub exists exactly when
  the real package cannot load under jsdom or would need a native runtime
  (gesture, reanimated, skia, webview). Everything the stub replaces is inert
  to the assertion; everything the test cares about is real.

## `createFlowApp`

Builds an app that renders a path against a routes map the way expo-router
would — group stripping, param capture, layout chain — and returns a
`FlowSession` per render.

```ts
import { createFlowApp } from '@lib/flow-test-harness';

const app = createFlowApp({
  routesRoot: 'app',
  routes: {
    'app/tamil/writing': () => import('./app/tamil/writing'),
  },
});

it('renders a screen by path', async () => {
  const session = await app.render('/tamil/writing');
  expect(session.getByTestId('canvas')).toBeInTheDocument();
});
```

## `FlowSession`

`app.render(path)` returns `RenderResult` plus faithful interactions addressed
by testID, each async and wrapped in `act`.

### tap

```ts
await session.tap('lesson-submit');
```

### type

```ts
await session.type('answer-input', 'ellam');
```

### layout

`layout` gives an element a box and lets react-native-web's own ResizeObserver
+ `getBoundingClientRect` path fire, so the component's real `onLayout` runs.
A Tamagui view measures through IntersectionObserver instead, polled every few
frames, so its `onLayout` follows within a frame or two (wait for it). The
setup file installs that observer before any test file loads Tamagui, which
switches its measurement off for good when the observer is missing at import.
Call `layout` before `stroke`, which reads the element's box.

```ts
await session.layout('canvas', { x: 0, y: 0, width: 300, height: 400 });
```

### stroke with a transform

`stroke` runs the same pipeline as the browser SDK: transform the path, sample
it, map fractions to page coordinates off the element's box, then fire pointer
down/move/up. Write the lesson's own reference stroke and see it accepted;
write a wrong one with a `transform` and see it refused with no ink.

```ts
import { transformPath } from '@lib/gesture-geometry';

const reference = activeGuide(); // fraction points from the …ActiveGuide path
await session.layout('canvas', { width: 300, height: 400 });

const wrong = transformPath(reference, { reverse: true });
const { samples } = await session.stroke('canvas', wrong);
expect(samples).toBeGreaterThan(0);
expect(session.getByTestId('ink')).toBeEmptyDOMElement();
```

### drag

`drag` is a straight-line stroke between two points — the gesture that is
actually a SHAPE needs `stroke`.

```ts
await session.drag('slider-thumb', { x: 0, y: 0.5 }, { x: 1, y: 0.5 });
```

### editor()

The editor is the real one. On web the app renders CodeMirror in an iframe
(`lib/webview/ReactNativeWebView.web.tsx`, the page from `editorHtml.ts` with the
real CM6 bundle), and flow tests run exactly that page inside jsdom.
`session.editor()` waits until it has hydrated, then drives it through the
bundle's own web test driver `window.__editor__`, the API the Playwright e2e
tier uses. Every change goes through CodeMirror, so the app receives the same
bridge messages a user's typing produces. No test sends a bridge message.

```ts
const editor = await session.editor();
await editor.select(bodyStart, editor.text().length);
await editor.type('return []');
expect(editor.text()).toContain('return []');
expect(editor.diffRanges()).toEqual(['return [1, 2]']); // CodeMirror's .cm-diff-change marks
```

### Rendering like the web export

`app.render` server-renders the route first, lets prerendered iframes finish
loading (and post whatever they post), and only then hydrates, which is the
lifecycle of the static web export. Pass `{ prerender: false }` only to isolate
a client-render question. With this default, the blank-editor bug (the frame's
only `ready` arriving before any listener, fixed in 297214d) fails a test
without anyone having predicted it. Two jsdom gaps are closed in
`src/vitest/frames.ts`: `iframe[srcdoc]` is loaded, and cross-frame messages
carry their `source`.

For a dynamic route that is not enough: the default server-renders with the path's own params, but the export holds one
template page per route (its `[param]`s prerendered as their own names) unless the route's `generateStaticParams` lists
the ids. `{ prerender: 'export' }` renders the page the export holds for the path (the route's own page when
`generateStaticParams` lists the path's params, else the template page, with no query) and hydrates it at the real URL,
which is a cold deep link. React reports a mismatch as an error event on the window, which fails the whole run unless a
listener has it; `hydrationErrors()` is that listener:

```ts
const caught = hydrationErrors();
await app.render('/tamil/uyir-1', { prerender: 'export' });
caught.stop();
expect(caught.seen).toEqual([]); // each message React reported
```

## Debugger: `breakAt` and `logAt`

A flow test can stop its own thread at a source line through `node:inspector`, with no flag and no external debugger:

```ts
import { breakAt, logAt } from '@lib/flow-test-harness';

const bp = breakAt('lib/device-lab/alert.ts', 25, ['index', 'pending.buttons[index].text']);
try {
  await session.tap('LabAlert-Button-1');
} finally {
  bp.release();
}
expect(bp.stops[0].values).toEqual({ index: 1, 'pending.buttons[index].text': 'Delete' });
```

- `file` is relative to the app root: the vitest project root, or `{ root }` as a fourth argument. The line is found in
  the transformed module by its source text, else by its number; the file must already be imported by the test.
- Each stop is `{ kind, file, line, function, values, stack, test, testFile }`. `values` holds each expression evaluated
  in the paused frame (`{ error }` when it throws); `stack` frames carry original file:line, mapped through the module's
  inline source map.
- `logAt(file, line, read)` is the logpoint twin: it records the same stop each time the line runs and the code carries on.
- Every stop is appended as one JSON line to `FLOW_DEBUG_OUT`, which defaults to `<package>/.flow-report/debug.jsonl`
  (cleared at the start of each run; a path you set yourself is appended to, never cleared).

## The run report

Every run through `flowTestConfig` (`test:flow`, `test:goldens`, `test:flow:debug`, pass or fail) ends with
`flow report: <path>` and writes `<package>/.flow-report/report.json` and a self-contained `index.html` (gitignored):

- top level: `schema` (`flow-report/1`), `verdict` (`passed`/`failed`/`interrupted`), `startedAt`, `finishedAt`,
  `durationMs`, `root`, `paths { json, html, debug }`, `env { goldens, theme }`, `summary { total, passed, failed,
  skipped, files, slowest[] }`, `errors[]` (files that failed to load, unhandled errors), `warnings[]` (stderr outside a test);
- per test (`tests[]`): `file`, `name` (the full Given/When/Then name), `verdict`, `durationMs`, `errors[] { message,
  stack, frames[] }` at source lines, `goldens[] { step, snapshot, golden, goldenExists, actual, expected, diff }` when
  `FLOW_GOLDENS=1` (the PNG paths are where `wf flow goldens` writes them), `stops[]` from `breakAt`/`logAt`, and
  `warnings[]` (the test's stderr, capped at 50). A warning fails its test (`src/vitest/warningGate.ts`, installed by the setup): a flow run passes with none, as a device harness run does.

Every path is relative to `root`. `flowTestConfig({ report: { dir, goldensDir } })` moves the report or the committed goldens.

In CI the report dir is published with `wf test publish .flow-report`, which uploads it as a run bundle and posts its
`HEALTH_RPT.TEST_RUNS` row; a local run posts nothing.

## Adding a double or a stub

Pick the replacement by what the thing is, in this order:

1. **Business logic or I/O behind a port** (a backend, transport, recorder):
   write a **double**, in the port owner's `testing` entry, never here. Never a stub, never `vi.mock`.
2. **A native runtime with nothing to run in jsdom** (a native module, a
   worklet or Skia renderer, Flow-typed entry points): write a **stub**, here.
3. **Anything else** (components, design system, third-party UI with a web
   build): render it for real. If it fails to load, fix the harness
   (resolution, interop, a jsdom shim in `setup.ts`), not the test.

The harness holds only stubs and its driver API. It never ships a feature's doubles
(`docs/solutions/architecture/hexagonal-architecture-by-package-type.md`, type 14).

### A double (in the port owner's `testing` entry)

- **Where.** A feature's doubles live in its `testing/` (a package feature exports it as `./testing`);
  a library's in `src/testing/`, exported as `./testing`. `src/doubles/` here still holds leetcode's
  `fakeAudioRecorder.ts` and `fakeVoiceEditTransport.ts`; they are debt and move to leetcode's `testing/`
  (`docs/plans/hexagonal-architecture/spec.md`, Phase 5). Add no new double here.
- **Typed against the port.** The double imports the port type from the owner's `domain/ports.ts`, so
  TypeScript checks the fit.
- **`createFake<Port>(config)`.** Config scripts the outcomes (`transcript`,
  `alternatives`, `failWith`, `levels`, `permission`). Defaults give the happy
  path.
- **Record, don't assert.** Expose `calls: string[]` (and payloads where a test
  needs them). The test asserts against them or spies on single methods by
  spreading: `{ ...createFakeVoiceEditTransport(), plan: vi.fn(...) }`.
- **Drivers for pushed events.** Anything the real adapter pushes gets an
  explicit driver method (`emitLevel(n)`, `editor.emit(msg)`), called inside
  `act`.
- **Wire it in** through the feature's override key:
  `app.render(path, { overrides: { <feature>: { <port>: fake } } })`. The
  feature's composition root falls back to the live adapter for every port the
  test leaves out.
- **Test it** with a Given/When/Then unit test beside it.

### Microphone permission

`createFakeAudioRecorder({ permission })` takes `true` (granted), `false` (denied) or `'prompt'` (never asked): the first
`requestPermission()` answers with `promptAnswer` (allow by default) and moves `recorder.permission` to `'granted'` or
`'denied'`; `start()` before that rejects as a phone does.

### A stub (`src/stubs/<package>.cjs`, or `.mjs` for default-only exports)

- **Add it once**, as an alias in `flowTestAliases()` (`src/vitest/flowTestConfig.ts`),
  so every package gets it. Never alias per package.
- **Shape rule.** The module object is the default export, with named exports as
  own properties. Vite's CJS interop can hand `import X from` the whole module,
  and it builds named imports by enumerating own keys, so a bare Proxy breaks
  them.
- **Inert where it can't be faithful, faithful where tests look.** Pass children
  and testIDs through. Drive real callbacks from DOM events, as the
  gesture-handler stub does from pointer events.
- **Add a row** to the table below, saying why the real package can't load.

## Harness stubs

One line each for why it exists — the harness replaces native runtimes, nothing
else. Hard rule: a stub stops at the native API or data level; whatever jsdom can render is rendered, so a golden
captures it (a canvas's paths, markdown, chart marks).

| Stub | Why it exists |
| --- | --- |
| `react-native-gesture-handler` | ships untranspiled Flow syntax jsdom cannot parse; drives the real gesture callbacks from DOM pointer events |
| `react-native-reanimated` | has no worklet runtime in jsdom; values and animations collapse to plain numbers |
| `@gorhom/bottom-sheet` | a native sheet needs native layout and gesture runtimes; renders passthrough Views with a `data-sheet` marker (and `data-active-offset-y` when the web-only pan window is set) |
| `react-native-safe-area-context` | supplies the insets and frame a device would: an iPhone 15 Pro's (59 top, 34 bottom) unless a test calls `setSafeAreaDevice('iphone-se' \| 'ipad-pro-11' \| 'none')`; the setup file resets it after every test |
| `react-native-screens` | ships Flow-typed entry points jsdom cannot parse; its web overlay is a plain wrapper |
| `react-native-markdown-display` | renders the markdown source as plain text |
| `lucide-react-native` | real icons: per-icon imports load for real, and barrel imports are rewritten to each icon's own module (the barrel costs seconds of transform per suite); only non-icon names fall back to an inert stub |
| `victory-native` | draws through skia, which has no jsdom runtime; chart logic outside the canvas stays real, marks are inert |
| `@shopify/react-native-skia` | draws through CanvasKit, which has no jsdom runtime; the Canvas is a real element holding an SVG, and each primitive (Path, Circle, Rect, Text, Group) is drawn as its SVG twin with its paint, and keeps its geometry as data attributes |
| `expo-router` | records navigation and mounts the current route through the router harness; inside a drawer, `useNavigation().getParent()` is the drawer and takes `OPEN_DRAWER`/`CLOSE_DRAWER`/`TOGGLE_DRAWER` |
| `expo-router/drawer` (.mjs) | the navigator is expo-router's runtime and its vendored react-navigation ships Flow syntax; the drawer is the real `react-native-drawer-layout` view (resolved from the package's own expo-router), closed until a screen or the drawer content opens it, so goldens show the screen as a phone does |
| `expo-audio` | cannot load under jsdom and is never exercised — the recorder port is injected via the double |
| `@react-native-async-storage/async-storage` | an in-memory map on `globalThis`, seeded and inspected through the storage harness |
| `@rn-primitives/*` (.mjs) | import-analysis cannot parse their JSX; `Slot`/`Separator` render children / nothing |

## Stroke instruction set

Flow tests and the browser SDK share one instruction set — points in `fraction`
or `px` units, plus `curve`, `samplesPerSegment` and
`transform { reverse, offset, scale, truncate, wobble, wobbleSeed }` — through
`@lib/gesture-geometry`. Every harness that draws strokes imports it, and none
keeps a copy: the geometry that can silently be wrong is tested once,
in the pure package.
## Golden checkpoints

A flow test that already reaches a visual state can freeze it:

```ts
await session.snapshot('stroke-accepted');
```

This writes `.flow-goldens/snapshots/<suite>/<name>.html` on every run (`wf flow run` points it at `.flow-goldens/<theme>/snapshots/`). The file holds the live CSSOM rules Tamagui and react-native-web inserted, the viewport meta, the body, and the boxes set by `session.layout`. Every flow test also gets a final-state checkpoint: after the test (and a short settle for queued renders) the harness writes `final-<test name, 48 chars>-<hash>.html`. Without the flag all of this is a no-op, so normal runs pay nothing.

Screenshots work as in Flutter's golden tests. Every test's last screen, and every `screenshot(name)` / `session.screenshot(name)` it takes, is frozen as HTML on every run. `wf flow run` then paints them in headless Chromium at 393x852 with animations frozen (`wf flow goldens`), and the flow report shows them in a Screenshots grid. Without a flag nothing is compared and only the light theme is painted. `--goldens` (`test:goldens`) runs the suite once per theme (`FLOW_THEME=light`, then `dark`, so the app really renders each theme) and compares each PNG with the committed golden: top two screens, 360 px wide, pixelmatch with a 0.01% changed-pixel threshold (small enough to catch one line of text). A difference or a missing golden fails the run. `--update-goldens` (`test:goldens:update`) writes them, and a full run also removes goldens no test produces any more. Each theme writes a sidecar JSON per screenshot and a gallery of expected, actual and diff to `.flow-goldens/<theme>/report.html`.

```sh
pnpm --filter mla-tamagui run test:goldens          # check; reports at .flow-goldens/<theme>/report.html
pnpm --filter mla-tamagui run test:goldens:update   # accept the current renders as goldens
```

Goldens live in `frontend/mla-tamagui/tests/goldens/<suite>/<name>.<scheme>.png`, each with its `.json` sidecar.

Rendered faithfully: layout, text, theme colours and SVG (react-native-svg is real, so Tamil guides and ink show). Not faithful: anything behind a native-runtime stub (Skia charts, the WebView editor), fonts the page does not load itself, and animations, which collapse to their end values. This is a manual gate: it needs Chromium, so it is not in pre-commit (see `docs/known-gaps.md`).
