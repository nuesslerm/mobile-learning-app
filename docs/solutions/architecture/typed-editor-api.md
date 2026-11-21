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
- The app reaches the page through this API for everything that acts on the document or the focus: `focus`, selection, `insertAt`, `typeText` (the `type` step), `undo`, `redo` and the config it can express; a refused call is logged (`editor.port.forward_failed`) and does nothing. What still travels as a script: `init`, the highlight and diff marks, an `updateConfig` the API cannot express, and `blur` on iOS only (the page's own script, until a person has watched the typed one on an iPhone; `ForwardingOptions.blurThroughApi`). The flow tests drive the same API (`session.editor()` makes calls and reads a document mirror); the page keeps one test hook, `window.__editorDebug__`, which exists only when the page html is built with the debug flag and which the contract suite uses to simulate a person's typing and pointer selection.
- `configure({language})` is one request that changes the language and nothing else: the text, revision, selection and undo stack do not move and nothing is emitted. The languages an engine offers are in `hello.capabilities.languages` (the vocabulary is `LANGUAGE_NAMES`); a language outside what it offers is `unsupported` with `what: 'language'`, and a language set once stays through `load`.
- What the host can do is what `hello` announces, derived from the host's optional methods: the `type` step and the `complete` step (and the `completion` event) when it has them, the `tree` request when it has `tree()`. An engine whose host lacks one answers `unsupported` (`step:complete`, `request:tree`, `config:completion`) and does not list it. The core requests (`CORE_REQUEST_NAMES`) every engine answers.
- The `complete` step accepts the completion the editor has open, at the live selection, like `type` it is the only step of its call and acts on the head (an older `at` is `stale`); nothing open is `ok` with no change. The `completion` event says a list opened or closed, once per change. `configure({completion: {enabled, entries}})` turns completion on and gives it the names it may offer (at most 200, word characters only).
- The `tree` request returns the syntax tree as plain nodes (`type`, `from`, `to`, `error?`, `children?`) stamped with the instance and revision it describes, with `parsedTo` and `complete` when the budget ran out first.
- `tree` parses in slices of at most `treeSliceMs` and gives the event loop a turn between slices, so an apply asked while a tree waits is answered first; the job stops at `budgetMs` or when the head moves, and says so with `complete: false` (the tree then describes the revision it began at, `base`). Nothing in the tree is read past `parsedTo`: `isUnread` says so and callers treat that text as plain text.
- Syntax questions are pure functions over a tree reply (`core/syntax`): `nodeAt`, `nodesInRange`, `statementAt`, `enclosingBlock`, with node names classified by explicit per-language tables (`SYNTAX_TABLES`), never by pattern. The client has them as sugar (`client.enclosingBlock(pos)` reads the text and a point-range tree, retries once when a keystroke lands between the two reads, then answers `stale`). The fake editor answers `tree: true` through two tiny parsers (`testing/treeParsers.ts`); what they leave out is listed there, and `syntaxTreeContract` is the suite every engine that announces `tree` runs.
- The `type` step is one keystroke (1 to 64 characters) at the live selection, through the editor's input handlers, so the editor's own rules (auto-closed brackets, re-indent) apply; it is the only step of its call, it acts on the head (`at` older than it is `stale`, `map` or not) and its `landed` is `[null]`. A host that has no `type` method does not announce it and answers `unsupported`.
- Events of a call arrive before its reply. A `changed` event's `from` that does not follow the mirror's revision means a lost message: resync.
- The engine owns the selection mapping: an empty caret stays before text inserted at it, a range stays inward, and a range wholly inside replaced text collapses to a caret after the replacement (CodeMirror's own mapping leaves a range turned inside out).
- Contract failures resolve as `{ok: false, error}`; transport failures reject at the engine and resolve as `transport` through the client.

## Adding to it

A new operation is a request name, a step op, an optional field, an event type or an optional host method, announced in `hello.capabilities`; an unknown one answers `unsupported`. Give it a case in `testing/contract.ts` and it runs on the fake, over both transports and on the real CodeMirror bundle.

## The page's syntax and suggestions

The page offers python, javascript and cpp (`hello.capabilities.languages`) and switches between them with `configure({language})`, in one compartment, without touching the text, selection, revision or undo history. It answers `tree` from the live state (`ensureSyntaxTree` in slices the engine paces) and, once `configure({completion: {enabled, entries}})` has turned it on, offers the `complete` step and the `completion` event; a load and a language switch keep the config. Completion is off until configured, and the app configures it from the problem list's Suggestions row (off by default).

## What proves it

`pnpm --filter @lib/code-editor-api test` (the suite on the fake, over a loopback and the emulated WebView, a 16,000-operation differential test against real CodeMirror state and history, a ChangeSet conformance test, the fake's tree parsers and the class tables against the real grammars) and the real-bundle flow tests: `cm6Engine.contract.flow.test.tsx` runs the editor contract and the tree contract for each language, `cm6Language`, `cm6Completion` and the host's `cm6Host.flow.test.ts` the rest. The browser runs `e2e/editor-language.lua` and `e2e/editor-completion.lua` show the switch and the list on a phone frame. Not provable there: typing through the DOM and IME, soft-keyboard focus, layout, Android bridge timing: the Chromium and Android editor-typing e2e cover those.
