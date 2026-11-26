# Pull request tab for a checkout

On the paired `rca` iPhone 18 Pro Max, open the connected desktop, long-press the desired checkout, choose **Source Control**, then **Pull request**. Installed Orca 0.0.68 and Expo Go both opened PR #103 for `dev`. The Expo capture shows its live checks, branch names, author, mergeability, description and file totals. The reference includes review threads, approvals, description editing and a bottom merge action that the Expo overview still needs. Expo currently opens the hosted checks and files pages. [Orca capture](orca-dev.png) · [Expo capture](expo-dev.png) · [Published iOS update](https://expo.dev/accounts/nuesslerm/projects/mla-tamagui/updates/0dbb9097-c1c4-4c6c-933e-44d394ecde07)

In the captured responses, the hosted PR reported 948 changed files. Orca displayed the 100 entries it had loaded. Check counts changed between captures. The selected tab region at physical y 346–513 differed in 2,495 of 221,760 pixels. This measures the tab strip only. [Pixel comparison](pixels.json) · [Gateway response on dev](live-dev.json) · [Orca recording](orca.mp4) · [Expo recording](expo-dev.mp4)

On `main`, the paired gateway returned an empty result. The published app showed **No pull request yet**, opened the link form, and returned to the same screen on **Cancel**. A fixture flow exercised an actual link and checked that it preserved the checkout’s other saved settings. [Empty state](expo-empty.png) · [Link form](expo-link.png) · [After Cancel](expo-cancelled.png) · [Recording](expo-link.mp4) · [Gateway response on main](live-main.json)

The fresh gateway lookups took 3.16 s for `main` and 4.12 s for `dev` in one local sample. The final iOS run observed the rendered PR after 11 s and the empty state after 5.3 s, including capture and text recognition. [Request timings](latency.json)

## Reproduce

Use the paired simulator and the published update above. The [Orca runbook](orca.lua) and [Expo dev runbook](expo-dev.lua) start with the populated PR visible. The [Expo link runbook](expo-link.lua) starts with the empty PR tab visible on main. Supply an existing absolute output directory. The [browser runbook](web.lua) starts on the paired feed in the exported app and exercises both checkouts.

```sh
WF_IOS_APP_ID=host.exp.Exponent wf ios test expo-link.lua --device "$RCA_UDID" --env "out=$CAPTURE_DIR" --test-output-dir "$RUN_DIR"
```

Both light and dark browser runs reached PR #103, changed tabs, opened the link form on main and cancelled it. One earlier live light run timed out while the hosted lookup was still loading. The final light run completed in 15.9 s end to end. [Light capture](web-light.png) · [Dark capture](web-dark.png) · [Dark recording](web-dark.mp4)
