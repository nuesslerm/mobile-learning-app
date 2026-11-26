# Code blocks in chat and Chat UI settings

The `rca` iPhone 18 Pro Max on iOS 27 ran Orca 0.0.68 and the Expo Go update in light appearance. The captures use the default chat font, 14 pt and default line spacing. [Open the Expo update](https://expo.dev/accounts/nuesslerm/projects/mla-tamagui/updates/90bb8769-7a3d-4cda-a7c6-b634c03617e9)

| Action | Installed Orca | Expo Go |
| --- | --- | --- |
| Open Settings → Chat UI | [Preview](orca-settings.png) | [Preview](expo-settings.png) |
| Copy the preview code | [Feedback](orca-copied.png) · [Recording](orca-copy.mp4) | [Feedback](expo-copied.png) · [Recording](expo-copy.mp4) |
| Wrap a long TypeScript line in a paired chat and copy it | | [Before](chat.png) · [Wrapped](wrapped.png) · [Recording](chat.mp4) |

## Reproduce on iOS

In Settings → Chat UI, run [settings.lua](settings.lua) with `wf ios test`, an absolute `out` screenshot directory and a fresh `--test-output-dir`. The runbook taps Copy, captures the checkmark and waits for the original icon to return. The coordinates target the named simulator at the default text size.

For the chat recording, create a disposable thread on the paired RCA project. Send the following user message through `message.dispatch` with `dispatchMode` set to `{"type":"defer_start"}`. Open the thread in Expo, run [chat.lua](chat.lua) with the same arguments, then archive the disposable thread.

````markdown
Code block clipboard and wrapping fixture.
```ts
const names = ["a  b", "c"];

  const aVeryLongIdentifierForHorizontalScroll = names.map((name) => name.trim()).join(" and the line continues across the viewport");
```
After the block.
````

## Pixel comparisons

At the same screen coordinates, the entire default Chat UI preview matched Orca in all 1,112,616 physical pixels, both at rest and with the copy checkmark visible. The previous Expo preview differed in 395,835 pixels. The comparison covers x = 48 through 1271 and y = 365 through 1273. The wider settings content still differs in 136,879 of 2,984,520 pixels. These measurements exclude the status bar and other system chrome. [Measurements](measurements.json)

In the paired Expo chat, wrapping the fixture changed 376,826 pixels. Unwrapping reproduced all 1,762,200 pixels in the original transcript crop. Copy changed 815 pixels in the control on each surface.

## Mobile web clipboard

The dark mobile browser opened the paired chat from the exported app, wrapped the sample, copied it and disabled wrapping. Settings exposed Copy without a wrap control. Both clipboard checks matched the expected code exactly, including indentation, the blank line and the two spaces inside a string. [Chat capture](web-chat.png) · [Chat recording](web-chat.mp4) · [Settings capture](web-settings.png) · [Settings recording](web-settings.mp4)

Use a `wf browser` device on the same loopback origin as the export. Serve [the comparison page](clipboard-check.html) on that origin. Run [the chat journey](web-chat.lua) or [the settings journey](web-settings.lua), open the comparison page with `#chat` or `#settings` respectively, then run [the clipboard check](web-clipboard.lua). The page compares the clipboard locally and displays the result.
