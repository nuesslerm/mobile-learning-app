# Expo workspace tabs on iOS

The named `rca` simulator ran Expo Go against a paired RCA service and executor. The [published iOS update](https://expo.dev/accounts/nuesslerm/projects/mla-tamagui/updates/a44bfa12-a786-42a7-8032-e5986d5fa432) opened the fixture checkout through the feed. The New Tab screenshot is from the preceding update with the same sheet layout.

| Tab | Observed on the phone and desktop |
| --- | --- |
| Terminal | The phone ran `sh v` in the checkout and displayed `checkout=fixture result=42`. The shell reported 22 rows and 50 columns while the keyboard was open. After leaving the checkout and returning to the running command, the same PTY reported 39 rows and 50 columns. |
| Browser | The first frame showed the executor-local fixture. Typing `expo42` updated the input and the page output. Tapping Send requested `/clicked?value=expo42` on the fixture server, which answered HTTP 200. The next frame displayed Received. |
| Markdown Note | New Tab created `untitled-3.md` alongside two pre-existing notes. The file contained `Saved from Expo` after Save, and the editor displayed that text after switching away and back. A subsequent desktop write set its contents to `Desktop revision 23`. The stale mobile save displayed a conflict and preserved the phone draft on reopening. A desktop read confirmed the desktop revision survived. |

[Device run results](device-results.json) · [Terminal recording](terminal-reattach.mp4) · [Browser typing](browser-typed.mp4) · [Browser callback](browser-click.mp4) · [Note conflict](note-conflict.mp4)

After a cold relaunch of the published update, reopening the checkout restored the note and its conflict notice. Closing all three resources removed their rows from the switcher. The desktop note remained on disk.
