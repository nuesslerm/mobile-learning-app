# One component contract, two private design systems

## Context

The coding agent can be drawn by two component sets, Orca and Tamagui, chosen in `di/CodingAgentContainer.tsx`. Three gaps keep the swap from being rigorous:

- The contract (`@lib/agent-ui-contracts`) covers 41 composite components but no plain controls, so screens reach for React Native's own: `Button` in 5 production files, `TextInput` in 3, `ActivityIndicator` in 1. Those draw the platform's look under both sets.
- 28 production files in the feature import `@lib/tamagui-design-system` directly. Those screens look the same under both sets.
- Nothing proves the two sets behave alike. The first Tamagui settings screens copied Orca's layout instead of using the design system's own row and header, and no test would have noticed either way.

Goal: every control and every styled surface of a coding-agent screen comes from the contract, both sets implement every contract entry, and a shared conformance suite plus a gallery prove parity and show both looks.

## Decisions

1. Scope is the coding-agent feature. LeetCode, Tamil, settings and the app shell keep importing `@lib/tamagui-design-system` directly; they have no second set. Extending the contract app-wide is a later decision.
2. "Private" means three things: each set's package exports only its component set and theme provider; only `di/` of the feature may import a set; `@lib/tamagui-design-system` is importable from `tamagui-agent-components` but not from the feature.
3. The contract gains only the controls the design system already has: `Button`, `TextField`, `Switch`, `Spinner` (the sheet is already an entry). There are no layout primitives: `View`, `Text`, `ScrollView`, `FlatList`, `Pressable` and `Image` stay allowed for arranging contract components, with colors read from `useAgentTheme()`.
4. The Tamagui set is built from the design system's existing composites (`ListItem`, `InsetGroup`, screen header, `Sheet`, `TextField`), and its reference for "looks good" is the app's own `/settings` and LeetCode screens, not Orca's metrics.
5. The Orca set stays pixel-identical to stock Orca. Its parity baselines are the gate for every step.
6. Non-visual `react-native` APIs stay allowed in the feature: `Platform`, `Keyboard`, `Dimensions`, `useWindowDimensions`, `PanResponder`, and types.

## Final state

```text
frontend/libraries/
  agent-ui-contracts/src/
    ports/
      Controls.ts              NEW  Button, TextField, Switch, Spinner
      AgentComponents.ts       composites; every entry documented with its behavior
      WorkspaceTabComponents.ts  grows: tab strip, terminal keys and input, browser bar, note toolbar
      AgentTheme.ts            (unchanged)
    core/                      contexts and preference state (unchanged)
    testing/
      conformance.tsx          NEW  one behavior spec per contract entry, run against any set
      gallery.tsx              NEW  every entry with fixed props, for both sets
  orca-design-system/
    package.json               exports "." only
    src/index.ts               orcaComponents, OrcaThemeProvider; nothing else
    src/adapters/controls/     NEW
    tests/conformance.test.tsx NEW  runs the shared suite
  tamagui-agent-components/
    package.json               exports "." only; no dependency on orca-design-system
    src/index.ts               tamaguiComponents, TamaguiThemeProvider; nothing else
    src/adapters/{controls,home,pairing,chat,composer,workspaces,sourceControl,workspaceTabs}/
    tests/conformance.test.tsx NEW  runs the shared suite

frontend/mla-tamagui/features/coding-agent/src/
  ui/        contract components for every control; no design system; no react-native control
  di/        the one place that names a set
  routes/gallery.tsx  NEW  dev-only route: the gallery under the selected set
```

## Enforcement

| Rule | Where | Blocks |
|---|---|---|
| `only-di-picks-a-component-set` (exists) | feature depcruise | either set outside `di/` |
| `feature-reads-no-design-system` | feature depcruise | `@lib/tamagui-design-system`, `tamagui`, `@tamagui/*`, `lucide-react-native` outside `di/` |
| `no-react-native-controls` | `.oxlintrc.json` override for `features/coding-agent/src/{ui,routes,state}/**` | `Button`, `TextInput`, `Switch`, `ActivityIndicator`, `Modal`, `Touchable*` from `react-native`, each with a message naming the contract entry to use |
| sets are sealed | each set's `package.json` `exports` and a unit test | deep imports; an export other than the set and its theme provider |
| sets are complete | TypeScript (`AgentComponents` has no optional entry) and a unit test | a set that falls back to the other set |
| sets behave alike | shared conformance suite | a set whose entry misses a role, label, callback or disabled state |

Each rule lands with a list of the files that still break it. The list only shrinks; a rule is done when its list is empty and deleted.

## Steps

### 1. Seal and prove (no visual change)

1. Write `testing/conformance.tsx`: for each of the 41 entries, Given fixed props, When rendered and driven by role and label, Then the same callbacks fire and the same accessible names exist. Run it in both packages. Expect the Tamagui set to pass for free where it still reuses Orca entries.
2. Restrict both packages' `exports` to `.`; add the sealed-set unit test.
3. Add the three lint rules with their file lists (9 files with a React Native control, 28 design-system files).
4. Add the gallery and its dev route; capture light and dark baselines of the Orca gallery.

### 2. Controls

1. Define `Controls.ts`: `Button`, `TextField`, `Switch`, `Spinner`, with the props the design system's own versions take, minus anything Tamagui-specific.
2. Implement in both sets; add conformance specs and gallery entries. The Tamagui set wraps the design system's components; the Orca set wraps what its composites already use.
3. Replace the React Native controls in the 9 files. The Orca parity baselines move only where a platform button was drawn; recapture those against stock Orca.

### 3. Empty the lists, one area per batch

Order: settings, home tabs, pairing, chat and composer, workspaces and source control, workspace tabs, legacy thread screens (`ThreadScreen`, `ThreadComposer`, `feed/*`, dispatch runs).

Per batch:

1. Move the area's direct design-system imports and Orca-specific styled views behind contract entries. Plain layout views stay in the screen.
2. Orca adapter: pixel-identical to before.
3. Tamagui adapter: built from design-system composites; rebuild the settings entries shipped earlier on `ListItem` and the screen header.
4. Remove the area's files from the rule lists.
5. Drop the area's entries from the `...orcaComponents` spread; the batch that empties it removes the dependency of `tamagui-agent-components` on `orca-design-system`.

### 4. Close

Delete the empty rule lists, the spread, and any legacy screen that no route reaches. Record the learning with `/cf-compound`.

## Verification, per batch

1. Unit: sealed-set and completeness tests.
2. Flow: the conformance suite under both sets; `pnpm --filter mla-tamagui run test:flow` with the area's journeys parameterised on the set.
3. Orca unchanged: `wf test ios-visual` against the Orca parity baselines and the Orca gallery baseline, golden updates disabled.
4. Other screens unchanged: the 22 LeetCode and Tamil iOS baselines, golden updates disabled.
5. Tamagui looks right: gallery and area screens in light and dark on a fresh simulator, reviewed side by side with `/settings` and a LeetCode screen, then saved as baselines.
6. Device: `wf ship eas --initial-route /coding-agent`, walk the area in both sets on a fresh simulator copied from `rca`, record it.
7. Land on `dev`, update issue #102.

## Risks

- Layout views stay in screens, so a screen can still hardcode a size or color that fits one set only. The gallery and the per-area baselines in both designs are what catch it.
- The terminal, browser and note tabs carry keyboard and focus fixes that live in the screen. Extraction must move views only.
- Text lookup on a fresh simulator returned no elements in the last run; device walks need `wf test prepare-ios --build-input` fixed or verified first.
