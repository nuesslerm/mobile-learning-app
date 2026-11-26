# RCA workspace tabs implementation plan

**Goal** — In one paired checkout, open Terminal, Browser and Markdown Note from Orca's New Tab sheet in both the unmodified Orca iOS app and the Expo app. A new phone connection can reopen existing tabs. Expo uses the T3-style API.

**Architecture** — The desktop executor owns shell processes, Chromium pages and files. RCA routes operations to the executor for the selected workspace. Persist tab membership through the existing layout/state owner. The Orca and T3 adapters translate their own identifiers, errors and stream envelopes at their boundaries. Expo depends on mobile tab operations through a port injected at its composition root.

## Proposed final structure

Paths below are relative to the repository root. Existing package directories retain their ownership. New capability folders do not move existing chat code.

```text
libraries/agent-api/ (unchanged)               shared contracts
  {agent-api-browser,exec-browser}/ (unchanged) browser subscription lifecycle and frame delivery
  {agent-api-terminal,agent-api-workspace,agent-api-state}/ (unchanged) PTY, file and layout contracts
backend/remote-chat-agent/ (unchanged)         workspace routing
  {group-browser,exec-channel}/ (unchanged)    propagate frame demand, cancellation and disconnect
  {group-terminal,group-workspace,group-state}/ (unchanged) existing owners of session operations
clis/ (unchanged)
  rca-exec/src/adapters/ (unchanged)           browser.rs · existing pty.rs and filesystem implementation
  orca-serve/ (unchanged)                     stock Orca pairing and protocol
    {map-browser,map-terminal,map-workspace,map-state}/ (unchanged) translate capture-backed calls
    tests/integration/ (unchanged)             browser.rs · terminal_pty.rs · layout_vault.rs
  t3-serve/ (unchanged)                       mobile facade
    core/src/{domain,usecases,rpc}/ (unchanged) tab identity, application operations and dispatch
    src/adapters/ (unchanged)                  RCA port implementation
    tests/integration/ (unchanged)             workspace_tabs.rs · terminal_pty.rs
  rca-serve/src/ (unchanged)                   existing serve-t3 composition and pairing entry point
  mimic/{cli,proxy-core}/ (unchanged)          transport capture and replay; extend only for measured gaps
tooling/mimic-drivers/src/orca/ (unchanged)    reference DirectRpcClient and decoded events
docs/plans/orca-mobile-interface/reference/ (unchanged)
  workspace-tabs/ (new)                       provenance, scenarios, sanitized captures and reports
frontend/libraries/ (unchanged)
  agent-ui-contracts/src/ports/ (unchanged)   tab picker and tab surface props
  orca-design-system/src/adapters/ (unchanged) SessionHeader.tsx · existing BottomSheet · NewTabSheet.tsx
frontend/mla-tamagui/features/coding-agent/src/ (unchanged)
  domain/workspaceTabs/ (new)                 typed tab refs, capabilities and operation ports
  data/t3/ (unchanged)                        workspaceTabs.ts and stream adapters
  state/workspaceTabs/ (new)                  list, select, restore, disconnect and drafts
  ui/workspaceTabs/ (new)                     TerminalTab.tsx · BrowserTab.tsx · MarkdownNoteTab.tsx
  di/ (unchanged)                             CodingAgentContainer.tsx
  routes/ (unchanged)                         resolve a typed tab in its checkout
  testing/ (unchanged)                        deterministic fixture ports for UI flow tests
```

## Delivery sequence

1. **Capture reference journeys before specifying missing API fields.** Pair an isolated simulator with a fixture desktop, leaving the normal rca pairing intact. Pin the Orca phone and desktop builds, record browser-enabled and disabled runs, and record creation/opening of a fresh note separately from opening an existing .md file. Capture the original action, sheet dismissal and resulting active tab. Record how closing the last tab behaves.
   - Use the existing `wf mimic proxy record` on the paired fixture transport. Keep T3 recordings in a separate set. Orca seals application frames inside WebSocket; ciphertext does not supply request or response schemas. Match the journey to the paired reference client sidecar and, on the fixture server we control, record the decoded method, parameters, response, ordered events and binary-frame metadata. Label handset, sidecar and candidate evidence individually. Retain lengths, ordering, acknowledgements and close reasons. Sanitize before persistence and use synthetic page content.
   - Replay supported transport sets upstream-off with `wf mimic proxy replay`; run `wf mimic proxy check` against a candidate where the recorded protocol supports it. Re-execute decoded Orca scenarios over a **fresh paired session** for conformance. A replayed encrypted socket from a prior handshake cannot prove compatibility. Record any unavailable happy path as a gap.
2. **RCA tab lifecycle and notes.** Inspect the observed layout operations and source of truth for a new note. Reuse existing checkout layout, workspace files and versioned Markdown save. Preserve chat-only records. Test create, enumerate, select, close, reopen, duplicate create after a lost reply, concurrent saves against one base version, and a checkout disappearing mid-save. Keep tab metadata and file operations consistent across retries. The reference mobile source at Orca revision 980c00247 tries untitled.md, then untitled-2.md on collision, then opens the created file. Verify the installed version's behavior with its capture.
3. **Terminal through both adapters.** Reuse the executor PTY, T3 terminal.attach/write/resize/close and Orca terminal mapping. Map a checkout tab to its underlying terminal even before there is a chat. Given two worktrees, type a command that prints its working directory and a computed value. Check visible output, resize, background/reconnect, Ctrl-C, close and continued operation of the other tab. Exercise scoped credentials and cross-worktree lookup.
4. **Browser through both adapters.** Reuse executor Chromium navigation, history, input and its existing dev-server address policy. On a host where the reference streams, capture tab creation, subscribe, first frame, resize/rotation, pointer and keyboard input, frame acknowledgement, reconnect, unsubscribe and page close. Derive frame codec, sequence and flow control from the successful trace. Keep bounded outstanding frames per subscriber and release resources on cancellation. Surface browser-disabled and missing-worktree results distinctly. Capture T3's existing preview.open, preview.resize, preview.close and preview.list and its page stream before choosing the T3 facade; the pinned T3 checkout already has preview RPCs. Mark RCA-specific additions explicitly when there is no observed T3 counterpart.
5. **Ship Expo after the backend journeys pass.** Add the + button beside the existing tab count. Reuse the shared sheet and match the workspace subtitle, agent tiles, ordering, icons, row actions and dismissal from the Orca capture. Preserve existing chat switching and connect the three agent tiles to real supported creation flows. Persist the active tab at checkout scope. Implement keyboard and ANSI terminal rendering, browser viewport/input, and note editing, draft retention, save/conflict and preview as observed. Restore on cold start and present the existing RCA-unreachable and worktree-unavailable states. Capture both themes on the same simulator dimensions. Check Expo SDK 58 documentation before implementation.
6. **Black-box release gate.** Use the public pairing endpoints and built binaries with a real executor, disposable Git worktrees, a PTY and a fixture web server. Run the notes, terminal and browser journeys separately through the Orca and T3 endpoints, then run them through the unmodified Orca app and the published Expo iOS update. An implementation-level handler assertion or a proxy replay by itself does not establish live device behavior. Publish recordings and reports, link the EAS update and track each batch in issue 102.

## Observable acceptance

| Journey | Evidence required |
| --- | --- |
| Open New Tab and select a kind | Reference and Expo recordings on rca show the same controls and selected checkout. Existing tabs reopen after reconnect without creating duplicates. |
| Markdown Note | Text typed on the phone survives reopen. A desktop edit between read and save produces a conflict that preserves both the desktop content and phone draft. |
| Terminal | The phone displays computed shell output and the expected worktree directory. A resized shell reports the new dimensions. Disconnect and reattach retains output without duplicate input. |
| Browser | An executor-local fixture page appears on the phone. Tap and type a unique value; the fixture server receives it and the phone shows the changed page. Back/forward, rotation, backgrounding and close are exercised. On a disabled host the user sees the observed unavailable state. |
| Direct Orca compatibility | The stock app pairs with our Orca endpoint and completes each successful journey. Each result links the matched reference method/response/stream and a live run with reference servers out of the path. |
| Expo API compatibility | The published Expo build completes the same journeys using the RCA-backed T3 endpoint. Trace provenance distinguishes T3 reference behavior from explicit mobile extensions. |

## Existing test entry points

From `clis/`, use `cargo test -p orca-serve --test integration layout_vault::`, `cargo test -p orca-serve --test integration terminal_pty::`, `cargo test -p orca-serve --test integration browser::` and `cargo test -p t3-serve --test integration terminal_pty::`. Extend these chains with visible effects and add T3 workspace-tab integration cases. Run the new phone journeys using `wf ios`, publish their reports via `wf test publish`, and publish the Expo update through `wf ship eas`. Keep a baseline run separate from newly added behavior.

## First backend verification

The existing Orca integration case for opening, reading and saving Markdown passed through the paired wire, service and executor. The initial T3 terminal attach was refused because macOS exposed the temporary repo under a symlinked /var path. With the fixture rooted at the canonical path, both T3 PTY integration cases passed, including resize, observed output, close and rejection of an outside directory. Keep the alias failure visible as a separate compatibility case.

The committed Orca v1.4.259 recording contains an error result for browser.screencast and browser.tabCreate, a successful acknowledgement and an error on unsubscribe. It provides no first-frame success evidence. Use the reference paired-browser rejoin scenario with a browser-capable fixture desktop for that capture.

The added Orca integration case new_markdown_note_preserves_existing_files_and_detects_an_edit_from_the_desktop passed. It drives create collision, numbered creation, open, read, conflicting save and reconnect through the public paired wire against a disposable desktop checkout.
