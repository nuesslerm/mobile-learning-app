# The /effort command on mobile

Installed Orca 0.0.68 (1) and Expo Go on the rca iPhone 18 Pro Max opened the same native-fixture chat. The Commands menu offered /model, /effort and /resume in that order. Picking /effort inserted it, focused the draft and opened the keyboard. Pressing Send preserved the draft, closed the keyboard and displayed “Could not open the effort picker.” The notice disappeared by the four-second check. Reads of the actual T3 thread before and after the Expo and browser runs returned identical message IDs, roles, text and model selection.

| Step | Installed Orca | Expo Go |
| --- | --- | --- |
| Open Commands | [Menu](orca-commands.png) | [Menu](expo-commands.png) |
| Pick /effort | [Draft](orca-draft.png) | [Draft](expo-draft.png) |
| Send | [Notice](orca-send.png) · [Recording](orca-send.mp4) | [Notice](expo-send.png) · [Recording](expo-send.mp4) |

On 1320 × 2868 captures, the first red text pixels after Send were at y=2371 in Orca and y=2372 in Expo. Within the unfiltered menu band (y=1800 through 2449), 44,511 of 858,000 pixels differed. The top card edge was eight pixels lower in Expo. [Pixel counts](pixels.json)

The live RCA-backed OpenCode catalog reported three models. The chat's selected gpt-5 had no catalog entry. The unavailable state above was verified on-device. For capable models, the client reads optionDescriptors from the model advertised by server.getConfig and writes the chosen option through thread.model-selection.set. Running the client parser on the [native T3 cassette](../../t3/native-open-chat/interactions.jsonl) returned the recorded five Opus and six fixture-provider choices. [Parsed choices](t3-catalog.jsonl)

Route flows covered opening a supported picker, selecting by label, applying a typed argument, rejecting an unknown choice and preserving rejected drafts. Adapter tests exercised canonical and legacy option representations, unrelated settings and a stale-model rejection. Supported option writes were verified through these tests. Only the unavailable case was exercised on the paired iPhone.

To reproduce on the paired device, open the native-fixture chat and choose Commands with the keyboard dismissed. Run the [pick](pick.lua) and [Send](send.lua) steps in that order, passing a writable capture directory as out. The [dark mobile web runbook](web.lua) starts in the paired thread. [Browser capture](web-notice.png) · [Browser recording](web.mp4) · [iOS EAS Update](https://expo.dev/accounts/nuesslerm/projects/mla-tamagui/updates/1561d3a3-5215-40d9-a106-8d119a1f1239)
