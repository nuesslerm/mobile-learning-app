# Design system audit

## Scope

Reviewed the Tamagui design-system package, its theme and native bridges, and all 21 entries in the Expo showcase catalog. The recording and the iPhone 17 Pro Max simulator informed the first visual pass on the showcase shell, list and component picker. The remaining entries and production callers were reviewed in source. Android, web, dark mode, and accessibility settings still need visual passes.

## Direction

Use Liquid Glass for the controls that float above an app surface, including the component picker trigger, navigation and sheet chrome. Keep text, lists, inputs and editor content legible on neutral surfaces. Use one row shape for flat lists and give rounded corners to an enclosing group only when the group is visibly a separate surface. Reserve color for primary actions, focus and semantic states. [Apple materials guidance](https://developer.apple.com/design/human-interface-guidelines/materials)

Tamagui owns shared layout, typography, semantic colors, surface spacing and specialized app content. Use Expo UI for platform-native presentation and controls when their behavior fits the feature. Compare equivalent states and content in the showcase so a platform difference is visible without having to infer it from different sample data. [Expo SDK 58 UI](https://docs.expo.dev/versions/v58.0.0/sdk/ui/)

## Findings and ownership

| Area | Finding | Direction |
| --- | --- | --- |
| Theme, type, spacing and icons | The default iOS preset fed the same blue into primary actions, focus and the accent alias consumed by neutral components. Several controls also bypass preset intent with explicit blue tokens. Typography, radii and spacing are shared, but the showcase repeated the navigation title and emphasized implementation labels over the example itself. | Tamagui owns the preset and semantic roles. Keep a quiet interaction surface separate from primary, focus, success, warning and destructive colors. Audit explicit blue in Button, Badge, dialogs and keyboard toolbar before changing their primary actions. |
| Basic controls | Text, Badge, Button, IconButton, Icon, Separator and Spinner cover the common primitives. The Expo counterparts use platform defaults, while Tamagui applies its own typography and feedback. | Start with one foreground hierarchy, consistent icon size and a clear primary/secondary distinction. Use Expo controls for native chrome where the platform supplies focus, feedback and accessibility. Keep Tamagui text, icons and status badges for shared content. |
| Inputs and selection | TextField, Switch, SegmentedControl and Chip are showcased. Tabs, ToggleGroup and Textarea are implemented but lack equivalent showcase coverage. Focus, selection and press colors need to be distinguishable in both hosts. | Prefer Expo UI for native binary controls and segmented selection where it meets the interaction contract. Keep Tamagui for cross-platform validation, tokens, chips and form layout. Exercise selected, disabled and focused states. |
| Lists and surfaces | The same comparison combined an opaque Tamagui group, blue row presses and the default native inset-grouped list. The picker could carry that treatment into the sheet. Card and StatTile represent separate content surfaces. The chart and admonition components live outside the showcase. | Use flat transparent rows with thin dividers for these examples. Keep enclosed cards for actual grouping. Use native list presentation and virtualization where appropriate, and retain Tamagui rows inside mixed feature surfaces. |
| Navigation and overlays | The package contains both a native header path and an in-content fallback. The showcase needs its header behind its inline Gorhom backdrop. AlertDialog, PromptDialog and Sheet already have native counterparts, with differing detents, keyboard behavior and background interaction. | Prefer native presentation for straightforward sheets and dialogs. Use Gorhom for inline placement or gesture behavior a feature needs across platforms. Keep a single backdrop and quiet close affordance, and validate the keyboard, accessibility and dismissal contracts independently. |
| Coding agent and keyboard | CodeBlock, Markdown, DiffView and Composer are showcased. The keyboard toolbar, menu panel and live editor integration have specialized layout and gesture requirements. | Keep these in Tamagui, using glass selectively for floating actions. Audit their contrast and focus in context before moving any of them into a native host. |

## First showcase pass

- The selected example now has the page's main heading. A compact glass control opens the component picker on supported iOS devices and uses the outline button elsewhere.
- The picker floats above the safe area with a grab handle, a centered title, and a single low-contrast options surface. The list scrolls within that surface while the title and close control stay in place.
- The comparison labels use smaller, quieter typography. The list example uses the same row copy in both hosts and shows the iOS list in its native plain style.
- Transparent groups have square edges. Grouped rows retain subtle separators, and row presses and sheet close controls use the muted surface.
- The app debug button is hidden on the showcase so it does not compete with the controls being reviewed.
- Navigation includes a live reveal drawer example. Expo Router owns swipes and dismissal, and Tamagui owns the drawer's labels, spacing and active destination.

Next, compare the focused and selected states for buttons and inputs across light and dark appearances, then review dialogs and the keyboard toolbar in the actual screens that use them. Use simulator screenshots for material and layering, flow tests for interaction, and the platform fallbacks for web and Android checks.
