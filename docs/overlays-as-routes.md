# Overlays as router-addressable routes

Bottom sheets and alert/prompt dialogs used to be imperative, ref-driven
siblings mounted next to the screen that opened them (`descriptionSheetRef
.current?.open()`, a `renameTarget` `useState`). That means they have no
address: they can't be deep-linked, can't be cold-started for a preview
(`--initial-route`), and can't be driven by the browser CLI except by
replaying the exact tap sequence that opens them.

This doc is the recipe for making an overlay a route instead, while keeping
its rendering primitive (`Sheet`, `AlertDialog`, `PromptDialog` from
`frontend/libraries/tamagui-design-system`) completely unchanged — this only changes *how the
primitive is invoked* (route vs ref).

Reference migrations (`frontend/mla-tamagui/features/leetcode`):

| Overlay | Primitive | Route |
|---|---|---|
| `ProblemDescriptionSheet` | `Sheet` | `/leetcode/[id]/description` |
| `RenameSymbolDialog` | `PromptDialog` | `/leetcode/[id]/rename` |
| `ResetCodeDialog` | `AlertDialog` | `/leetcode/[id]/reset-code` |
| `CodeHistorySheet` | `Sheet` | `/leetcode/[id]/history` |
| `UnsavedChangesDialog` | `AlertDialog` | `/leetcode/[id]/unsaved` |

Several overlays are **not** migrated yet and are still siblings mounted by
whatever opens them, driven by component state rather than a route, among
them `frontend/mla-tamagui/features/leetcode/ui/AlternativePicker.tsx` and the
voice-edit `frontend/mla-tamagui/features/leetcode/ui/VoiceReviewSheet.tsx`
(both mounted by `ProblemEditorScreen.tsx` from the voice-edit phase), and the
debug log sheet `frontend/mla-tamagui/app-shell/DebugLogSheet.tsx` (a
`useState` in `DebugFab.tsx`; the old dev-tools `DebugSheet` is gone).
Migrate them with this same recipe. `LanguageSheet` (`ui/LanguagePicker.tsx`) is also a
sibling, rendered at the end of `ProblemEditorScreen`'s own tree on purpose:
it does not portal, so it only covers what comes before it (see its comment).

## 1. Route placement and naming

Nest the overlay route under the screen it overlays, as a sibling folder of
the dynamic segment: `app/leetcode/[id]/description.tsx` sits next to
`app/leetcode/[id].tsx`. Expo Router supports a dynamic file and a same-named
dynamic folder side by side (`[id].tsx` matches `/leetcode/123`; `[id]/
description.tsx` matches `/leetcode/123/description`) — this is the
documented nested-dynamic-route pattern, not a workaround.

Colocating with the feature keeps an overlay discoverable next to the screen
it belongs to and avoids two different physical directories contributing to
the same URL namespace. A cross-feature overlay with no single natural
parent screen (a global "are you sure you want to sign out" dialog, for
example) can use a top-level `app/(overlays)/<name>.tsx` group instead — the
group segment is elided from the URL, so it doesn't change the addressing
scheme, only where the file lives.

## 2. Screen options: `transparentModal`, not `modal`

Register the route in the nearest `_layout.tsx` above it, the one whose Stack
owns the screen it overlays. Screen names are relative to that layout. The
leetcode overlays are registered in
`frontend/mla-tamagui/features/leetcode/routes/_layout.tsx`, the leetcode
Stack's layout, which `frontend/mla-tamagui/app/leetcode/_layout.tsx`
re-exports, not in the root layout, so the name is `[id]/description`
(`leetcode/[id]/description` would match nothing there). A route with no
`_layout.tsx` between it and the root goes in the root
`frontend/mla-tamagui/app/_layout.tsx` under its full path, as
`coding-agent/[threadId]/settings` does. That route is an exception to §1,
not a model. Its thread screen is
`frontend/mla-tamagui/app/(app)/(content)/coding-agent/[threadId].tsx`,
registered in the drawer's `(content)` Stack
(`frontend/mla-tamagui/app/(app)/(content)/_layout.tsx`), while the overlay
file sits outside both groups, so the root Stack presents it over the whole
drawer and two directories both serve `/coding-agent/...`. Each overlay gets its own
`Stack.Screen` with three options inline; there is no shared options constant:

```tsx
<Stack.Screen
  name="[id]/description"
  options={{
    presentation: 'transparentModal',
    headerShown: false,
    animation: 'none',
  }}
/>
```

- **`transparentModal`** (not `modal`): the new screen is presented on top of
  the current one, which stays mounted and visible underneath — this is what
  preserves the invoking screen's state (typed code, scroll position, form
  values) instead of navigating away from it. `modal` would also keep the
  previous screen mounted but paints an opaque card over it, hiding it.
- **`animation: 'none'`**: the DS primitive already owns its entrance/exit
  animation (gorhom's sheet slide, or the alert dialog's fade/zoom). Without
  this the screen-level push transition doubles up with the primitive's own
  animation and the sheet visibly snaps partway open.
- **`headerShown: false`**: the primitive renders its own handle/header.

## 3. The overlay component: controlled, not ref-driven

`Sheet`, `AlertDialog`, and `PromptDialog` all share one controlled contract:
an `open` boolean plus `onOpenChange(open)`. `AlertDialog` adds `onConfirm`
and `PromptDialog` adds `onSubmit(value)`; none of them has an `onClose` or
`onCancel` prop, and there is no `Sheet.Root`. Use that contract instead of a
`forwardRef` handle, turning `onOpenChange(false)` into the overlay's own
`onClose` (`frontend/mla-tamagui/features/leetcode/ui/ProblemDescriptionSheet.tsx`):

```tsx
export function ProblemDescriptionSheet({ problem, onClose }: Props) {
  return (
    <Sheet
      open
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      snapPoints={[90, 60]}
      testID="ProblemDescriptionSheet"
    >
      {/* unchanged content */}
    </Sheet>
  );
}
```

A dialog overlay also passes `modal={false}`, because the `transparentModal`
route is already the modal layer; `ResetCodeDialog`, `UnsavedChangesDialog`
and `RenameSymbolDialog` all do. `PromptDialog` defaults to `modal` true and
hands it to Tamagui's dialog, which then portals to the root host
(`hostName: "root"`), and a route-hosted dialog that portals there swallows
every touch (the comment above `AlertDialog` in
`frontend/libraries/tamagui-design-system/ui/alert-dialog.tsx`). `AlertDialog`
itself is built on gluestack and mounts its overlay host inside the route, so
it currently ignores the prop, but its doc comment still asks for `false` and
the leetcode dialogs pass it.

`snapPoints` is a `number[]` of screen-height percentages, largest first.
`open` is always `true` here — the component only exists while its route is
mounted, so "open" and "mounted" are the same event. The route wires `onClose`
to `dismissOverlayRoute(id)`, and so does the effect that leaves when the
problem does not exist:

```tsx
// features/leetcode/routes/[id]/description.tsx — thin, no business logic
export default function LeetCodeProblemDescriptionRoute() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const problem = id ? ProblemCatalog.getById(id) : undefined;

  useEffect(() => {
    if (!problem) dismissOverlayRoute(id);
  }, [problem, id]);

  if (!problem) return null;

  return <ProblemDescriptionSheet problem={problem} onClose={() => dismissOverlayRoute(id)} />;
}
```

`dismissOverlayRoute` (`frontend/mla-tamagui/features/leetcode/ui/dismissOverlayRoute.ts`)
calls `router.back()` when `router.canGoBack()`, and otherwise
`router.replace('/leetcode/<id>')` (or `/` without an id). A bare
`router.back()` does nothing on an overlay that was cold-started or
deep-linked, because there is no history entry to pop: the user lands with the
overlay open and its close control does nothing (issue #164). That is exactly
the stand-alone case §4 and §7 rely on. The fallback is `replace`, not `push`,
so the overlay does not stay in the stack behind the editor.

This covers the simplest case: everything the overlay needs (`problem`) is
re-derivable from the route param alone, with no data owned by the invoking
screen.

## 4. When the overlay needs data or a result from the invoking screen

The harder case: `RenameSymbolDialog` needs to know *which* symbol to
rename, and — on confirm — needs to mutate the *live* editor state (text,
cursor selection) that belongs to a different component instance than the
overlay route. Once the overlay is its own route, it's a different React
tree; a ref can't reach across that boundary, and route params are one-way
(they flow into the overlay, not back out of it) and must be
URL-serializable (no functions).

The fix is a tiny cross-instance request channel:
`frontend/libraries/tamagui-design-system/lib/overlayRequestStore.ts` exports
`createOverlayRequestStore<TPayload>(name)`, a Zustand store holding one
pending `payload` (or `null`) plus `request(payload)` / `clear()`. Bundle any
callbacks the invoking screen needs into the payload itself:

```ts
// frontend/mla-tamagui/features/leetcode/state/renameSymbolRequestStore.ts
export type RenameSymbolRequest = {
  problemId: string;
  target: string;
  onConfirm: (newName: string) => void;
  onCancel: () => void;
};
export const useRenameSymbolRequestStore =
  createOverlayRequestStore<RenameSymbolRequest>('leetcode.renameSymbol');
```

The invoking screen calls `request(...)` **synchronously before**
`router.push`, so the store is populated by the time the overlay route
mounts and reads it. The live example is reset-code, `handleResetCode` in
`frontend/mla-tamagui/features/leetcode/ui/ProblemEditorScreen.tsx` with
`useResetCodeRequestStore`:

```tsx
const handleResetCode = React.useCallback(() => {
  if (!problem) return;
  useResetCodeRequestStore.getState().request({
    problemId: problem.id,
    onConfirm: () => { /* snapshot, then reset this screen's own editor text */ },
    onCancel: () => { /* log and leave the code alone */ },
  });
  router.push(`/leetcode/${problem.id}/reset-code`);
}, [problem, lang, snapshot]);
```

Rename's in-app trigger is **not wired yet**. `handleRenameSymbol` in
`ProblemEditorScreen.tsx` only logs `editor_v2.rename_symbol_request`; nothing
calls `useRenameSymbolRequestStore.getState().request(...)` or pushes
`/leetcode/<id>/rename`, so the toolbar's rename action never opens the
overlay, the route is reachable only by URL, and the store's payload is always
`null`. When it is wired it takes the same shape: request
`{ problemId, target, onConfirm, onCancel }`, then push
`/leetcode/<id>/rename?symbol=<encoded symbol>`.

The overlay component reads the same store, invokes whichever callback fired,
then clears the request so a stale payload can't be replayed on the next
open:

```tsx
export function RenameSymbolDialog({ initialTarget, onDismiss }: Props) {
  const request = useRenameSymbolRequestStore((s) => s.payload);
  const clearRequest = useRenameSymbolRequestStore((s) => s.clear);
  const target = request?.target ?? initialTarget ?? '';
  // onConfirm/onCancel: call request?.onConfirm/onCancel, then clearRequest(), then onDismiss()
}
```

Also accept the identifying piece of the payload (`symbol`, here) as a
**route param fallback** (`initialTarget`). That is what makes the route
usable stand-alone: a cold deep link, or `--initial-route
"/leetcode/two-sum/rename?symbol=foo"`, has no live `request()` behind it
(nothing called it from within the app this session), so the store's
`payload` is `null` — the dialog falls back to the URL param and still
renders something meaningful, and its confirm/cancel become no-ops instead of
throwing. A dialog that needs *no* extra data at all (`ResetCodeDialog` —
just confirm/cancel) skips the URL-param fallback entirely; a route param
fallback is only needed when the dialog has a payload-shaped piece of state
worth previewing without a live request.

Unlike a plain Zustand store driving a globally-mounted overlay, this store
also carries a one-shot payload plus confirm/cancel callbacks, because the
trigger and the overlay are two separate route entries instead of permanent
siblings.

## 5. Barrel and route re-export

The overlay component is exported from the feature's public barrel, same as a
screen: `frontend/mla-tamagui/features/<name>/index.ts` for an in-app feature
such as `leetcode`, and the package's `main` entry, `src/index.ts(x)`, for a
workspace-package feature (`coding-agent` uses `src/index.tsx`, which
exports its `ThreadSettingsSheet`). Its overlay route
`app/coding-agent/[threadId]/settings.tsx` is the package case for this
barrel rule only; its placement is the exception noted in §2. The thin
`app/` route is a one-line re-export of the feature's own route module
(`export { default } from '@/features/leetcode/routes/[id]/description'`, or
`@feature/<name>/routes/...` for a package feature), which mounts the overlay;
the `app/` file never imports the feature's internals.
Each feature is one package (no `-mocks` stub to keep in parity). Request-store
hooks stay internal (relative-imported within the feature package) unless
something outside the feature genuinely needs them. A feature's entry points
(barrel, `routes/*`, `testing`):
`docs/solutions/architecture/hexagonal-architecture-by-package-type.md`.

## 6. testIDs

The `testids:extract` / `testids:check` inventory belonged to the first app
and was deleted with it, and lint does not check testIDs anywhere in the app or
its feature packages (the testID rules were dropped with the move to oxlint,
`frontend/.oxlintrc.json`), so nothing regenerates a testID inventory. Give the
overlay's primitive and its interactive elements a `testID` the same as any
other screen (`ProblemDescriptionSheet` sets `testID="ProblemDescriptionSheet"`):
the e2e scripts (`frontend/mla-tamagui/e2e/verify-overlay-deeplink.mjs`,
`verify-sheet.mjs`) select overlays by those IDs, so renaming one means
updating them too.

## 7. Verifying

- **Deep link / cold start**: with the app running (cold start Expo Go on the
  bare update link `exp://u.expo.dev/<projectId>/group/<group>` first: its
  manifest answers 404 under a `/--/` path while Expo Go is not running,
  `e2e/warm-app.lua`), `wf android open-link
  "exp://u.expo.dev/<projectId>/group/<group>/--/leetcode/two-sum/description"` (or
  `/rename`, `/reset-code`; `wf android open-link` has no route option, so the
  route goes in the URL after `/--/`) opens the overlay on top of nothing
  (single-entry stack) — the standalone-preview case from §4. A cold start
  straight into the overlay is an update published with `wf ship eas
  --initial-route <route>`.
- **In-app trigger**: navigate to `/leetcode/two-sum`, tap the trigger
  (info button / Reset button) and confirm the underlying editor is still
  mounted and its state intact once the overlay is dismissed (gesture,
  backdrop tap, or the primitive's own Cancel/Confirm). Rename has no in-app
  trigger yet (§4); reach it by URL.
- **Web**: `wf browser` against a local `expo export --platform web` (as
  `frontend/mla-tamagui/e2e/verify-overlay-deeplink.mjs` does through
  `frontend/mla-tamagui/e2e/lib/servedApp.mjs`, which serves `dist/` under the
  `/mla-tamagui` base path), never GitHub Pages —
  `transparentModal` is expected to keep the previous route's content visible
  underneath on web too, since React Navigation keeps every stack entry
  mounted regardless of platform; only the *native* modal chrome is
  iOS/Android-specific.
