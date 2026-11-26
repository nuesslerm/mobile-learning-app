# Stock iOS reference

Orca mobile 0.0.68 on iOS 27 paired through the Mimic reverse proxy with an isolated Orca desktop 1.4.255. The disposable checkout had one local browser tab when the app opened it. Opening Terminal from the + sheet launched a shell in the fixture directory. The command echo RCA-$((20+22)) printed RCA-42. Choosing Markdown Note created untitled.md in the desktop checkout. Selecting the tab showed the editor toolbar and document. The agent catalog on this reference host produced a longer New Tab list than the three-agent layout in the design screenshot.

[Host-specific menu](host-agent-menu.png) · [Markdown editor](markdown-editor.png)

A paired Mimic sidecar then exercised the reference desktop through the loopback proxy. It selected each of the three fixture tabs and checked the active tab ID, closed it and checked its absence, then reopened its kind. Terminal started a new shell, files.open reopened the same Markdown document and Browser reopened the fixture URL. [Decoded responses](tab-lifecycle.json) · [Correlated WebSocket exchange order](lifecycle-transport.json)
