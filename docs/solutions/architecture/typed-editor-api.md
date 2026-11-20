---
title: "The typed editor API: one revision-stamped contract for the code editor, its fake and its clients"
date: 2026-11-20
category: architecture
module: code-editor-api
problem_type: architecture_pattern
component: development_workflow
severity: medium
applies_when:
  - "Adding an editor operation, a client of the editor (toolbar, test driver, runner, voice edit) or a new editing engine"
tags: [editor, codemirror, webview, contract, hexagonal]
---

# The typed editor API (`@lib/code-editor-api`)

Text is canonical. Every client edits it through seven typed requests on one engine port, each answered with the document revision it applied to and the one it produced. Issue 86 (B1) is where it was decided; the later items (toolbar, syntax tree, semantic ops, block view, voice edit) add requests, step ops and events, never new engines.

## Shape

- `ports/`: the wire types (`Requests`, `Reply`, `EditorEvent`, `EditorError`), `EditorEngine` (`request`, `listen`: what a client calls) and `DocumentHost` (what an engine core needs from an editor).
- `core/`: pure. Changes and mapping (`mapPos` is CodeMirror's rule over the individual change list), the revision log, `planBatch`, the validators, `createEditorEngine(host)` with its FIFO queue, `createEditorClient` (sugar that never rejects), `createDocumentMirror`.
- `adapters/`: `serveEditor` (page side) and `toRemoteEngine` (host side) over `@lib/webview-rpc`, the only imports of it (the preset's `libraries` option).
- `testing/`: `createFakeEditor` (a string host under the same core), `editorContract` (the suite), `withFaults`.
- The CodeMirror host is `features/leetcode/code-editor/engine/cm6Host.ts`, served beside the old messages from `cm6Bundle.src.ts`.

## Rules a client can rely on

- A call is one transaction, one revision (when the text changed) and one undo step. Steps are simultaneous and positions are in the document at `at`. Overlapping steps are refused: CodeMirror would merge them silently.
- `at` older than the head is `stale` unless the call says `stale: 'map'`, which maps ranges through the revision log and answers `range-lost` or `text-mismatch` (with `expect`) rather than editing the wrong text. A lost reply is retried with the same `at`: the retry is `stale`, so nothing applies twice.
- Events of a call arrive before its reply. A `changed` event's `from` that does not follow the mirror's revision means a lost message: resync.
- The engine owns the selection mapping: an empty caret stays before text inserted at it, a range stays inward, and a range wholly inside replaced text collapses to a caret after the replacement (CodeMirror's own mapping leaves a range turned inside out).
- Contract failures resolve as `{ok: false, error}`; transport failures reject at the engine and resolve as `transport` through the client.

## Adding to it

A new operation is a request name, a step op, an optional field, an event type or an optional host method, announced in `hello.capabilities`; an unknown one answers `unsupported`. Give it a case in `testing/contract.ts` and it runs on the fake, over both transports and on the real CodeMirror bundle.

## What proves it

`pnpm --filter @lib/code-editor-api test` (the suite on the fake, over a loopback and the emulated WebView, a 16,000-operation differential test against real CodeMirror state and history, a ChangeSet conformance test) and the real-bundle flow test `cm6Engine.contract.flow.test.tsx`. Not provable there: typing through the DOM and IME, soft-keyboard focus, layout, Android bridge timing: the Chromium and Android editor-typing e2e cover those.
