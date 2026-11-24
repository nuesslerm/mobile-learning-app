# Liquid glass top bars and the morphing settings menu: reference UX (section H of issue 86, 22 Nov)

Two references from the owner, with this request: "also go and create another section to recreate the following ux in our app using expo go 58s morphing popover menu which should be usable in expo go. and make the topbar of our leetcode screen use the Liquid Glass Grouped Toolbar Item (often referred to as a Toolbar Platter / Grouped Glass Pill). like the claude app. in general i would like our topbars to use the ios liquid glass elements instead of tamagui. we should update this in our design system".

## 1. The recording: LeetCode's iOS editor-settings menu (25 s, iOS 26, 1284x2778)

Frames: `rec2/f-menu-open.png` (menu open), `rec2/f-format.png` (Format Code popover), `rec2/f-morph.png`, `rec2/f-fontsize.png`, `rec2/f-symbolbar.png`, contact sheets `rec2/sheet-01.png` to `sheet-09.png` (two frames per second, six per sheet). Read them.

The top bar of the editor screen: a round glass back button (chevron), a muted "Saved" cloud label, and at the right a glass pill `Python3 ≡` (language name and a sliders glyph).

1. Tap the pill: it **morphs** into a popover menu (iOS 26 Liquid Glass morph: the pill's glass blob grows and blurs into the menu surface, anchored at the top right, and collapses back into the pill on dismiss). The menu is a rounded glass card, dark, with SF Symbols at the left and a trailing chevron on submenu rows:
   - `AA  Font size: 16px  >` (submenu)
   - `Show Whitespace` (a space-bar glyph; toggle)
   - `Line wrap` (a wrap glyph; toggle, checkmark at the left when on)
   - a hairline separator
   - `Tabs: 4  >` (submenu)
   - `Symbol Bar` (a keyboard glyph; opens a sheet)
   - `Format Code` (a sparkles-wand glyph; opens a confirmation)
2. Tap a **toggle** (`Show Whitespace`, `Line wrap`): the menu **stays open**, the checkmark appears or goes, and the editor behind changes at once (whitespace shows as dots; long lines wrap or run off). Tapping again toggles back.
3. Tap `Font size: 16px >`: the menu's content **replaces itself in place** with a header row (`AA  Font size: 16px  v`, the chevron now pointing down) and a list `12px 13px 14px 15px 16px (check) 18px 20px`; the card grows. Tapping the header returns to the root list; picking a size applies it and (as recorded) the menu returns to the root with the new value. `Tabs: 4 >` does the same with `2`, `4` (check), `8`.
4. Tap `Symbol Bar`: the menu dismisses (collapses back into the pill) and a full-height **sheet** rises: header row `Reset` (pill, disabled until something changed), centred title `Symbol Bar`, `Done` (pill) at the right; a card with a row `Show Symbol Bar` and a green switch, the caption `Shown above the keyboard while editing code.`; a card with eight rows `Group 1` ... `Group 8`, each with its characters in monospace (`#$^%`, `\@~_`, `&|?!`, `+-*/`, `'"`=`, `{}[]`, `()<>`, `.:,;`) and a drag handle (three lines) at the right; the caption `Each group holds up to 4 characters. Leave a group empty to hide it. Drag the handle to reorder.` Dragging a handle lifts the row and reorders (Group 2 moved below Group 3; `Reset` becomes enabled). `Done` closes the sheet; the symbol bar above the keyboard in the editor shows the new order.
5. Tap `Format Code`: the menu **morphs into a small confirmation popover** in the same place: title `Format current code?`, body `Your code will be reformatted.`, and one wide red-text button `Format`. Tapping `Format` formats and the popover collapses back into the pill; tapping outside cancels.

The rest of the editor screen in the recording is the console UX of section G (action bar with `Console`, reset, run, `Submit`, symbol bar and keyboard); it is not part of this request.

## 2. The screenshot: the Claude iOS app's top bar (`reference-claude-topbar.png`)

The iOS 26 **grouped glass toolbar** ("toolbar platter", "grouped glass pill"): a row of native navigation-bar items floating on the content, each group on its own piece of Liquid Glass: at the left a round glass button (a hamburger/sidebar glyph); in the centre a wide glass capsule holding a **two-line title** (`EAS update expo…` and a muted subtitle `mobile-l… · No hold…`); then a round glass button holding a spinner (a progress indicator); and at the right one **grouped pill** of two bar items that share a single glass background (a git-branch/pull-request glyph and a `...` more-menu glyph), separated by a gap from the spinner. The content scrolls under the glass and the bar has no opaque background (scroll-edge effect). That is what the owner wants for the top bar of the leetcode screen, and "in general" for the app's top bars, through the design system.

## What the owner asked for, in their words

- recreate this UX (the settings menu) in our app "using expo go 58s morphing popover menu which should be usable in expo go";
- the topbar of the leetcode screen should use the iOS Liquid Glass grouped toolbar item, like the Claude app;
- "in general i would like our topbars to use the ios liquid glass elements instead of tamagui. we should update this in our design system".
