---
title: "Flipping the native-headers setting while a dashboards screen is mounted throws DashboardsPortsProvider missing in a flow test"
status: open
area: frontend
paths:
  - frontend/libraries/tamagui-design-system/lib/nativeHeaderStore.ts
  - frontend/mla-tamagui/features/dashboards/ui
opened: 2026-11-23
last_verified: 2026-11-23
links: []
---

# Flipping native-headers while a dashboards screen is mounted

Found while building H9 (23 Nov 2026): a flow test that sets the native-headers store off (before render or live) on a dashboards screen on iOS fails with `DashboardsPortsProvider missing` and a React concurrent-render error. The cause is not found: a store flip may re-render the tree outside the dashboards provider (the root theme bridge), or it may be a harness artefact. The off-tests for the dashboards screens were dropped; the classic path stays covered by the settings off-table and the web flows. Worth a look if the Appearance toggle crashes with a provider-backed screen stacked behind it on a phone.
