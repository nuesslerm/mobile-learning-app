---
status: open
area: t3-serve
paths:
  - "packages/contracts/src/server.ts"
opened: 2026-11-20
last_verified: 2026-11-20
---
# t3 serve answers `server.getTraceDiagnostics` with the not-found error, which no recording holds

The one recorded success of `server.getTraceDiagnostics` (crawl cassette) is the pinned server reading its own `server.trace.ndjson`. t3 serve
writes no trace file, so it answers the shape the pin's schema declares for that case (`error: Some({kind: "trace-file-not-found", message})`,
every count zero). That shape was authored from `ServerTraceDiagnosticsResult` in `packages/contracts/src/server.ts`, not recorded.

Consequence: a replay of the crawl row differs from the answer in `error` and in the counts. The row is served, and its evidence entry must say
the answer is authored. Closing the gap needs a recording of the pinned server started with no trace file.

The same holds, with less weight, for `subscribeDiscoveredLocalServers` (`configuredUrlProbing` is answered `false`, unlike the recording, because t3 serve probes no URL)
and for the history rows, whose cpu and memory columns are the host's readings where the pinned server reports its own process tree.
