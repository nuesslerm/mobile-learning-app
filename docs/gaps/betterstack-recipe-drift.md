---
status: open
area: logging
paths:
  - "docs/solutions/workflow/betterstack-query-recipes.md"
  - "docs/archive/README.md"
  - "clis/telemetry/**"
opened: 2026-08-20
last_verified: 2026-10-20
links: []
---
# The BetterStack query recipes in docs/solutions use a form that returns nothing

## What is broken

1. The `curl -X POST ... -d "<SQL>"` form fails against the query host: `Code: 62. DB::Exception: Empty
   query.` The same SQL as a URL parameter works: `curl -G --data-urlencode "query=$Q"`.
2. The documented payload shape is the mobile source's. Backend OTLP rows nest attributes:
   `JSONExtractString(JSONExtractRaw(raw,'attributes'),'<key>')`, and the service is
   `JSONExtractString(JSONExtractRaw(JSONExtractRaw(raw,'resource'),'attributes'),'service.name')`.
   The top-level path returns zero rows, indistinguishable from "nothing was logged".

## Why blocked

Not blocked; the fix moved out of CLAUDE.md into docs/solutions and the corrected forms never followed.

## What unblocks it

Rewrite both solutions docs with the `-G --data-urlencode` form and the nested backend paths, and prove
each recipe returns a line you just caused (`wf vm logs` should use the same shape).

## Re-checked 2026-10-20

Both recipe docs still filter backend rows on top-level `JSONExtractString(raw,'module')` and never use the nested `attributes`/`resource` paths. `wf vm logs` already handles the nesting (vm_logs.rs:112-113, 209-240) and returned 3 backend rows on 20 Oct, so only the docs lag. The 'POST -d returns Empty query' half did not reproduce through wf, whose plain/text POST body works, and a direct curl comparison was not permitted from this session.

Out of date above:

- Point 1 ("The `curl -X POST ... -d` form fails ... Code: 62 Empty query") did not reproduce through wf. `wf vm logs` sends the docs' ✅ shape, POST with a `Content-Type: plain/text` raw body (vm_logs.rs:188-196), and returned rows on 20 Oct. At most, the failure applies to a POST without that header; a direct curl test was blocked here.
- "`wf vm logs` should use the same shape": it already reads module/service from the nested paths (vm_logs.rs:112-113, 209-240) and lifts nested `attributes` into its output (vm_logs.rs:266-271). Only the two docs need the fix.
- "The top-level path returns zero rows" holds for module, service and metadata. Backend rows do carry top-level `level` and `message` (wf vm logs printed INFO and the message from them, vm_logs.rs:272-280), so the docs' level/message extraction is correct.
- frontmatter `paths: clis/telemetry/**`: the query code is clis/deploy/src/vm_logs.rs (and clis/deploy/src/report_run.rs:54-86), so add those.
- last_verified 2026-10-04: re-verified 2026-10-20.
