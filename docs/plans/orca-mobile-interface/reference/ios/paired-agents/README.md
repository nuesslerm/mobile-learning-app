# Paired Agents and returning from a chat

The [Expo Go update](https://expo.dev/accounts/nuesslerm/projects/mla-tamagui/updates/ba1cb7aa-254d-4c38-bae0-b1b8ef564f0e) and installed Orca 0.0.68 opened the same saved photo conversation from the Agents tab on the rca iPhone 18 Pro Max. Both showed the two color swatches and the final reply. Back restored the agent list. [Orca list](orca-agents.png) · [Expo list](expo-agents.png) · [Orca chat](orca-chat.png) · [Expo chat](expo-chat.png) · [Expo after Back](expo-returned.png)

Orca had two connections to the fixture. Its [desktop picker](orca-desktops.png) reported 24 agents on each connection. Selecting one gave 24 rows under Done. Expo had one paired desktop. The authenticated RCA extension returned 21 completed and three interrupted agents. It sends one checkout entry per workspace and orders the activity by update time. Cleared and never-started panes are omitted. Each linked checkout must belong to the configured executor. A foreign registration with the same path is covered by the adapter flow. [Gateway result and five local timings](gateway.json)

| Compared region | Pixels different |
| --- | ---: |
| Full Agents screen | 341,574 / 3,785,760 |
| Agents header, physical y=180–299 | 0 / 158,400 |
| Bottom bar and inset, physical y=2615–2867 | 0 / 333,960 |
| Full opened chat | 428,032 / 3,785,760 |
| Expo list before and after Back, below y=179 | 0 / 3,548,160 |

[Exact RGBA counts](pixels.json). The full-screen comparison includes differences in saved read state, the reference's two-desktop filter, and the system clock. Expo's first group begins at y=405 and Orca's at y=537. Expo still lacks the native bulk and swipe actions. The chat header, tab count, timestamps and composer controls also differ.

### Reproduce

1. Open a paired Feed in the normal Coding agent destination. Tap Agents and wait for the host to finish loading. On the two-desktop Orca fixture, select Mobile reference in the desktop picker.
2. Run [select-agent.lua](select-agent.lua) with `wf ios test` on rca. Supply a capture directory in `--env out=<absolute-path>` and the selected row in `--env agent_point=<x%,y%>`. The first row on this Expo build was at 31%,16.3%. The filtered Orca row was at 26%,21.2%.
3. Verify the saved photo chat, its two attachments, and the list after Back. A row with a matching T3 conversation opens that conversation. When there is no match, the checkout's active chat is used if available. The fixture's first interrupted agent exercised that fallback. [Expo recording](expo.mp4) · [Orca selection](orca-select.mp4) · [Orca return](orca-return.mp4)

The dark exported browser app paired with the same RCA gateway, opened the first agent, displayed the final photo reply and returned to Agents. Its chat Back button still has the accessible name “Back to worktrees”. [Browser runbook](web.lua) · [Recording](web.mp4) · [Returned list](web-dark-returned.png)
