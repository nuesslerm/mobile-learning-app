# Unpaired mobile Feed on the RCA simulator

These captures use the simulator named `rca` (iOS 27.0, 1320 × 2868 physical pixels), light appearance, English, and an empty host catalog. The reference is Orca mobile at `980c0024741812bd7ae934c0c22144d9926a784e`. Separate simulator bundle identifiers preserve the existing paired Orca installation.

[Feed](feed.png) opens on a fresh install. The tab bar reaches [Agents](Agents.png), [Usage](Usage.png), [Automations](Automations.png), and [Settings](Settings.png). Returning to Feed and tapping **Pair desktop** opens the [pairing screen](pair-scan.png). The [recording](navigation.mp4) shows the transitions.

The [MLA Feed capture](mla-feed.png) renders the feature’s English unpaired Feed and its injected design-system components inside the same native Orca parent, with the same layout, tab bar and renderer. This isolates the component comparison. Verification of MLA’s app entry, independent navigation chrome, other locales, and connected journeys is pending.

Run this from the repository root to reproduce the image comparison. The only mask is the system return-to-previous-app label in the top left. iOS varies that label with the last foreground bundle.

```sh
frames=docs/plans/orca-mobile-interface/reference/ios/unpaired
wf test ios-visual unpaired-feed --exact --max-diff-ratio 0 \
  --reference "$frames/feed.png" --capture "$frames/mla-feed.png" \
  --ignore 30,113,420,152 --out .agents/tmp/orca-feed-comparison
```

The exact comparison passed with zero differing pixels across 3,770,550 compared pixels.
