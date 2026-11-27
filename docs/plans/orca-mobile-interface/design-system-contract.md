# One skin contract, two private skins, app-wide

## Context

The coding agent can be drawn by two component sets, Orca and Tamagui, chosen in `di/CodingAgentContainer.tsx`. The rest of the app cannot: LeetCode, Tamil, settings, dashboards, the service report and the app shell import `@lib/tamagui-design-system` directly.

| Area | Files importing the design system |
|---|---:|
| LeetCode | 68 |
| Coding agent | 28 |
| App and app shell | 22 |
| Tamil | 17 |
| Settings | 10 |
| Dashboards, service report | 8 |

54 of those files lay out with Tamagui's `XStack`/`YStack`, and 502 lines use its tokens (`gap="$3"`, `bg="$color2"`). In the coding agent, 34 files also import `react-native` components, among them `Button` (5), `TextInput` (3) and `Text` (11), which draw the platform's look under both sets.

Goal: every screen in the app is built only from components injected through one skin contract. Both skins implement every entry. No feature can import a design system, and no feature can use a React Native control or text.

## Decisions

1. Both skins ship for good as a user setting. Parity is enforced by tests permanently, and every new screen works under both.
2. Scope is the whole app: coding agent, settings, Tamil, LeetCode, dashboards, service report, app shell.
3. One contract, `@lib/skin`, provided once at the app root. Generic entries sit at the top level; a feature's own composites sit in a group (`skin.agent.*`). The design setting moves from the agent's Appearance screen to the app's Settings.
4. The Orca skin gets its own implementation of every generic entry. It does not reuse Tamagui components. There is no stock Orca screen for LeetCode or Tamil, so those Orca looks are derived from Orca's palette, type, radii and spacing, and reviewed by eye.
5. Layout goes through the contract too: `Row` and `Stack` are entries with semantic props (`gap`, `padding`, `align`, `justify`, `surface`). Tokens such as `$3` and `$color2` do not appear in a feature.
6. "Private" means: a skin package exports only its skin and theme provider; only the app's composition root imports a skin; `@lib/tamagui-design-system` is importable only from `@lib/tamagui-skin`; `tamagui`, `@tamagui/*` and `lucide-react-native` are importable only from a skin.
7. Banned in every feature, the app and the app shell: `Text`, `Button`, `TextInput`, `Switch`, `ActivityIndicator`, `Modal` and `Touchable*` from `react-native`. `View`, `ScrollView`, `FlatList`, `Pressable`, `Image` and non-visual APIs (`Platform`, `Keyboard`, `Dimensions`, `PanResponder`, types) stay allowed.
8. The Tamagui skin is built from the design system's own composites. Its reference for "looks good" is today's app: LeetCode, Tamil and `/settings` must not change under the Tamagui skin.
9. The Orca skin stays pixel-identical to stock Orca in the coding agent.
10. Parity proof is every flow test run under both skins, plus a gallery of every entry. There is no separate conformance suite.
11. Order: the coding agent first, then settings, Tamil, dashboards and service report, LeetCode, app shell. The lint rules land app-wide on day one with shrinking file lists.

## Names

| Today | Becomes |
|---|---|
| `@lib/agent-ui-contracts` | `@lib/skin` (`frontend/libraries/skin`) |
| `@lib/orca-design-system` | `@lib/orca-skin` |
| `@lib/tamagui-agent-components` | `@lib/tamagui-skin` |
| `AgentComponents`, `useAgentComponents()`, `AgentComponentsProvider` | `Skin`, `useSkin()`, `SkinProvider` |
| `AgentTheme`, `useAgentTheme()`, `AgentThemeProvider` | `SkinTheme`, `useSkinTheme()`, `SkinThemeProvider` |
| `orcaComponents`, `tamaguiComponents` | `orcaSkin`, `tamaguiSkin` |
| `ComponentSetPreference`, stored key `componentSet` | `SkinPreference`, stored key `skin` (the old key is read once and migrated) |

## Final state

```text
frontend/libraries/
  skin/src/
    ports/
      Layout.ts        Row, Stack
      Controls.ts      Text, Button, IconButton, TextField, Textarea, Switch, Spinner,
                       SegmentedControl, ToggleGroup, Tabs, Chip, Badge, Icon, Separator
      Surfaces.ts      Card, ListItem, InsetGroup, StatTile, Admonition, CodeBlock,
                       Markdown, DiffView
      Overlays.ts      Sheet, PageSheet, SheetPager, AlertDialog, PromptDialog, MenuPanel
      Chrome.ts        ScreenHeader, CompactHeader, CompactTabs, FloatingBar, SearchPill,
                       KeyboardToolbar, ComposerBase
      Charts.ts        BarChart, LineChart, PieChart, StackedBarChart, TimeSeriesChart
      agent/           the coding agent's composites (today's AgentComponents)
      Skin.ts          the whole contract: generic entries + { agent }
      SkinTheme.ts     colors and type a skin provides
    core/              SkinProvider, SkinThemeProvider, preference state
    testing/gallery.tsx  every entry with fixed props
  orca-skin/           exports ".": orcaSkin, OrcaThemeProvider
    src/adapters/{layout,controls,surfaces,overlays,chrome,charts,agent}/
  tamagui-skin/        exports ".": tamaguiSkin, TamaguiThemeProvider
    src/adapters/{layout,controls,surfaces,overlays,chrome,charts,agent}/
  tamagui-design-system/  unchanged inside; importable only from tamagui-skin

frontend/mla-tamagui/
  app/_layout.tsx      the one place that names a skin: reads the stored choice, provides it
  app/gallery.tsx      dev-only route: the gallery under the selected skin
  features/*/src/ui/   useSkin() only; no design system; no react-native control or text
  features/settings/   owns the Design setting
```

Icons: a feature names an icon by kind (`icon="chevron-right"`); each skin maps kinds to glyphs.

## Tamagui helpers and the compiler

What the app has today (`frontend/mla-tamagui/babel.config.js`, `metro.config.js`):

- The Tamagui babel plugin runs with `disableExtraction: true`. Extraction was turned off as a correctness fix: with it on, elements carrying a theme token disappeared from the static export. The plugin also lists `components: ['tamagui']`, while the app imports from the single `@tamagui/*` packages through `primitives.tsx`.
- So no compile-time style extraction or view flattening happens now. Moving components behind the contract takes away no optimisation the app currently gets.

What the contract would cost if extraction were turned back on later:

- The compiler optimises a JSX element only when it can see, at build time, that the element is a Tamagui component and that its style props are constants. A component read from `useSkin()` is a runtime value, so no call site in a feature can be flattened or extracted.
- Inside `@lib/tamagui-skin` the adapters import Tamagui components directly, so the compiler still works there, but only for styles that are constant in the adapter. A prop passed straight through (`gap={props.gap}`) is dynamic and stays a runtime style.

How the plan keeps the door open:

- `Row`, `Stack`, `Text` and the controls take a small fixed set of semantic values (`gap: 'sm' | 'md' | 'lg'`, `surface: 'plain' | 'group' | 'raised'`). The Tamagui skin implements them as `styled()` variants, which Tamagui resolves without building a style object per render and which the compiler can extract.
- Tamagui's helpers stay fully usable inside the skin: `styled()`, variants, tokens, `Theme`, `useTheme`, media queries, `pressStyle`. They stop being usable in features, which is the point of the ban.
- The theme still switches through Tamagui's `Theme`, so a light/dark change does not re-render through the contract.

The alternative that keeps call-site optimisation is to pick the skin at build time: `@lib/skin` becomes a module the bundler resolves to one skin, features import components statically, and the compiler sees every element. The price is that the design stops being a user setting; each skin is its own bundle or EAS channel. Decision 1 chose the runtime setting, so this plan injects at runtime and keeps the optimisation inside the skin. The step 1 spike measures the difference; if it is large, revisit decision 1.

What is lost for good: token shorthand and responsive props written at the call site in a feature (`$gtSm={{ gap: '$4' }}`), and any future flattening of feature-level layout. Step 1 measures render time on the LeetCode list and editor with one screen converted, before the rest follows.

## Enforcement

| Rule | Where | Blocks |
|---|---|---|
| `only-root-names-a-skin` | depcruise, app and every feature | `@lib/orca-skin`, `@lib/tamagui-skin` outside `app/_layout.tsx` and test harness setup |
| `no-design-system-outside-a-skin` | depcruise, whole frontend | `@lib/tamagui-design-system`, `tamagui`, `@tamagui/*`, `lucide-react-native` outside the skin packages |
| `no-react-native-controls` | `.oxlintrc.json`, app, app shell, every feature | the components of decision 7, each message naming the skin entry to use |
| skins are sealed | each skin's `package.json` `exports` and a unit test | deep imports; an export other than the skin and its theme provider |
| skins are complete | TypeScript (`Skin` has no optional entry) and a unit test | a skin that reuses the other skin's component |
| skins behave alike | every flow test runs under both skins | a skin that breaks a journey |

Each rule lands with a list of the files that still break it. A list only shrinks; a rule is done when its list is empty and deleted.

## Steps

### 0. Turn the compiler back on, before any skin work

1. Reproduce the failure that made `disableExtraction: true` necessary (elements with a theme token missing from the web export), with Metro's cache cleared.
2. Suspects to test, one at a time: the babel plugin lists `components: ['tamagui']` while Metro's `withTamagui` lists `['tamagui', '@lib/tamagui-design-system']`, so two compiler passes run with different settings; the app imports from single `@tamagui/*` packages, not the `tamagui` barrel; the installed 2.7.7 against the current release.
3. Gate: extraction on, `expo export --platform web` renders every route (`e2e/app-routes.lua`), the 22 iOS baselines pass, and the plugin's timing log reports optimised and flattened elements on the LeetCode list.
4. Record render time and bundle size as the baseline every later step is compared with.

If extraction cannot be made correct on this version, say so in issue #102 and continue: the skin work does not depend on it.

### 1. Rename, seal and measure (no visual change)

1. Rename packages, types and hooks as in Names, in one commit with no other change.
2. Move the provider and the stored choice to the app root; keep the coding agent working as before.
3. Run every coding-agent flow file under both skins.
4. Add the three import rules app-wide with their file lists; seal both packages' exports.
5. Add the gallery route; capture Orca and Tamagui gallery baselines in light and dark.
6. Spike: add `Row`, `Stack` and `Text`, convert the LeetCode list screen only, and compare render time and bundle size with before. Record the numbers in `docs/audits/2026-09-02-token-diet-decision-log.md`. A regression stops the plan for a decision.

### 2. Coding agent

1. Add `Button`, `TextField`, `Switch`, `Spinner` to the contract in both skins.
2. Area by area (settings, home tabs, pairing, chat and composer, workspaces and source control, workspace tabs, then thread, feed and dispatch runs): move direct design-system imports and Orca-styled views behind entries, keep Orca pixel-identical, build Tamagui from the design system's composites, remove the area from the `orcaSkin` spread in the Tamagui skin.
3. Rebuild the Tamagui settings entries already shipped so they match `/settings`.

### 3. Generic entries, one family per batch

Controls, surfaces, overlays, chrome, charts. Per family: define the port from what the features actually pass today, wrap the design system's component in the Tamagui skin, write the Orca version, add both to the gallery.

### 4. Features, smallest first

Settings (10 files), Tamil (17), dashboards and service report (8), LeetCode (68), app and app shell (22). Per feature:

1. Replace design-system imports with `useSkin()` entries, and `XStack`/`YStack` with `Row`/`Stack`.
2. Run the feature's flow tests under both skins.
3. Tamagui skin: the feature's existing iOS baselines pass with golden updates disabled.
4. Orca skin: capture, review and save baselines.
5. Remove the feature's files from the rule lists.

### 5. Close

Delete the empty rule lists and the `orcaSkin` spread; remove the dependency of `@lib/tamagui-skin` on `@lib/orca-skin`. Record the learning with `/cf-compound`.

## Verification, per batch

1. Unit: sealed and complete tests for both skins.
2. Flow: `pnpm --filter mla-tamagui run test:flow` with the touched feature's files under both skins.
3. Tamagui skin unchanged: the 22 LeetCode and Tamil iOS baselines and the Tamagui gallery baseline, golden updates disabled.
4. Orca skin unchanged in the agent: `wf test ios-visual` against the Orca parity and gallery baselines, golden updates disabled.
5. New Orca looks: light and dark on a fresh simulator copied from `rca`, reviewed, then saved as baselines.
6. Device: `wf ship eas --initial-route <route>`, walk the batch in both skins on the fresh simulator, record it.
7. Bundle size and render time against the step 1 numbers; both skins now ship in one bundle.
8. Land on `dev`, update issue #102.

## Risks

- Size. This rewrites about 150 files and adds roughly 45 Orca components that exist only to give LeetCode and Tamil a second look. LeetCode alone is 68 files, including the editor and its keyboard toolbar.
- `Row`/`Stack` with semantic props cannot express everything 502 token lines do today. Where they cannot, the answer is a named entry, not a style escape hatch; expect the contract to grow during the LeetCode batch.
- Native header and keyboard toolbar are tied to navigation and focus. They move last, and only their views move.
- Text lookup on a fresh simulator returned no elements in the last run; fix or verify `wf test prepare-ios --build-input` before the first device walk.
