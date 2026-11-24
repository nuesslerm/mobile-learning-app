# @lib/tamagui-design-system

The design system of the app: Tamagui primitives, composites, theme and icons.

## Top bars (`composites/native-header`)

`ScreenHeader` is the one call site of a top bar; a layout uses `useScreenHeaderOptions` for the same bar as Stack.Screen options. Both take a `ScreenHeaderSpec`:
title, subtitle, back, `actions` plus one `menu` (one piece of glass) or `groups` (one piece of glass each), `busy`, `transparent` and a `testID`.

Two renderers draw a spec:

- the native bar (react-native-screens header items, glass on iOS 26), where `nativeHeaderActive` says so: platform iOS, the persisted native-headers setting on, and the screen opted in with `glass`;
- the Tamagui header (`ClassicScreenHeader`) everywhere else, with today's ids and DOM.

`transparent` applies to the native bar only: `headerTransparent`, a top scroll edge effect and no header background. A list under it adds `useScrollUnder(true)` to its ScrollView so it scrolls under the glass; both are empty on Android and web.

A menu is a tree (`HeaderMenuSpec`, validated by `validateMenuTree`): the native bar draws it as a UIMenu, Android and web as `MenuPanel`. `keepOpen` rows recreate a native menu, which closes its open submenus.

Rules: `Stack.Toolbar` is never mounted (it cannot express the grouping and keep-open the specs need); every `unstable_` option name is written in `composites/native-header` only, so a rename is one place.
