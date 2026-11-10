---
status: open
area: wf-browser
paths:
  - "clis/wf-kit/src/args.rs"
  - "clis/wf-kit/src/help.rs"
  - "clis/harness-browser/src/browser.rs"
opened: 2026-10-09
last_verified: 2026-10-20
links: []
---
# A refused `wf browser <command>` line prints a usage line without the command

## What is broken

The old navigate verb (`navigate --name phone --bogus x`) exited 2 with clap's error, then `Usage: wf browser --name <name>`.
The usage line left out `navigate`. `wf ship mla-tamagui --bogus` prints `Usage: wf ship mla-tamagui`, so the commands
that parse inside `browser::run` build their clap command without the sub-word. Seen in the device-lab
before and after run (`docs/audits/benchmarks/device-lab-browser-cli-before-after.md`); the pre-migration
binary printed one line from `wf` itself.

## Why blocked

Not blocked. `parse_spec` names the clap command from the program name alone.

## What unblocks it

Give `parse_spec` the command words (`browser navigate`) for the commands that `browser::run` dispatches, with
a black-box case in `option-errors.integration.test.ts` that expects the verb in the usage line.

## Re-checked 2026-10-20

Ran the wf binary built from HEAD 982839ee9. The navigate verb with `--name phone --bogus x` still printed `Usage: wf browser --name <name>`, and the list verb with `--bogus` printed `Usage: wf browser`. Only the run verb (today `wf browser test`), which has its own help file, prints its sub-word. The usage name comes from help::command_path (clis/wf-kit/src/help.rs:80-88), called at clis/wf-kit/src/args.rs:120. command_path stops at the longest prefix in the help table, and the table has no entries for the browser subcommands. No commit since 2026-10-09 addressed it, and no flow test or CI run covers it.

Out of date above:

- "`wf ship mla-tamagui --bogus` prints `Usage: wf ship mla-tamagui`": it now prints `Usage: wf ship [ARG]...`, so ship drops its sub-word too and no longer works as the counter-example.
- "`parse_spec` names the clap command from the program name alone" (Why blocked): since 927cef342 (2026-10-09 16:16, before this gap was recorded at 22:04), clis/wf-kit/src/args.rs:120 names the command with help::command_path(argv) (clis/wf-kit/src/help.rs:80-88). That is the longest argv prefix with a help-table entry. Browser subcommands other than `run` have no entry (clis/harness-browser/src/help/ has only wf__browser.txt and wf__browser__run.txt), so they fall back to `wf browser`. help.rs belongs in the gap's paths.
- "a black-box case in `option-errors.integration.test.ts`" (What unblocks it): that file was deleted in 78392dc4e (2026-10-10, TypeScript wf black-box suite removed). The case now belongs in the Rust flow binary, in clis/wf-harness/tests/flow/browser.rs or next to the analogous `wf android lease --bogus` case at clis/wf/tests/flow/surface.rs:208.
- "Give `parse_spec` the command words": a second fix exists now. Either pass the dispatched words (`browser <cmd>`) to the bin_name, or add help-table entries for each browser subcommand so command_path finds them.

## Re-checked 2026-10-20 (harness API H5)

The browser half is fixed by the H5 rename: every `wf browser` verb now has its own help entry (`wf browser <verb> --help`),
and a refused line prints one JSON line carrying the verb's own usage, e.g. `wf browser hierarchy --bogus` answers
`{"ok":false,"command":"hierarchy",...,"error":"unexpected argument '--bogus' found","usage":"wf browser hierarchy [--device N]"}`
with exit 2; the table verbs do the same from the command table (`clis/wf/tests/flow/surface.rs` checks every one).
An unknown verb (the old ones too, since H8) exits 2 with one JSON line listing the verbs there are. What is left is
the other nouns' clap usage line (`wf ship --bogus`); close this gap through `/cf-compound` once that is checked or
split out.
