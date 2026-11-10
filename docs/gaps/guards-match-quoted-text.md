---
status: open
area: agent-guards
paths:
  - "scripts/guards/**"
  - "libraries/hook-runtime/src/guard.mjs"
  - "libraries/hook-runtime/src/deny.json"
  - ".agents/config/permissions.json"
opened: 2026-10-04
last_verified: 2026-10-20
links: []
---
# Command guards regex-match text, so a mention of a blocked pattern is refused

## What is broken

- The old agent-approve-guard refused any command that merely *mentioned* a blocked command (a commit
  message quoting one had to go through `git commit -F <file>`). That half is fixed: its package is
  deleted and the approve guard is now `libraries/hook-runtime/src/guard.mjs`, which tokenises the
  command (`splitCommands`, `stripWrappers`, and `commandsIn`, which also expands `bash -c` and `eval`
  bodies), matches a rule's `command` only against each simple command's first token, and every args
  pattern in `libraries/hook-runtime/src/deny.json` is anchored. `git commit -m "do not run git reset --hard"`
  now passes; a real `git reset --hard`, or one inside `$( )`, is still blocked.
- What still matches quoted text is the `condition` scripts: `runCondition` hands each one the whole
  command as `GUARD_COMMAND`. `scripts/guards/block-raw-playwright.mjs` tests its playwright regex
  against that whole string, so any Bash call with a `node` segment is refused when another segment,
  such as a `cat > f <<EOF` heredoc body or a commit message, merely contains `require('playwright')`.
- `scripts/guards/block-secret-exposure.mjs` also reads `GUARD_COMMAND`, but its own parser skips quoted
  text, so a commit message quoting a leak passes. It still reads heredoc bodies and comments as commands:
  `cat > f <<EOF` whose body has the line `wf secret get KEY | head -c 4` is refused, and so is
  `cat f # note; wf secret get KEY | head -c 4`.
- `block-raw-playwright.mjs` also blocks any `node` command whose arguments name a `.js` file whose
  content matches its playwright regex, so `node --check` on a file that has playwright strings only inside shell-script
  literals is refused, though `--check` never executes.

## Why blocked

Both over-matches fail closed, so impact is low, and narrowing them needs real parsing; a wrong answer
is exploitable in the direction that matters. The guard's allowlist is tested against the whole
`GUARD_COMMAND` too, and that direction fails open: a mention of `libraries/browser-client` anywhere in
the call, even in a trailing comment, exempts a real raw-playwright `node` run. A script fed to `node`
on stdin also passes: `node < /tmp/x.cjs` and `cat /tmp/x.cjs | node` run a raw-playwright file the
file scan never reads, because `splitCommands` drops redirect targets and the scan reads only the
`node` segment's own tokens. A quoted assignment re-points a script path: the scan expands `$VAR` from
`VAR=` assignments found anywhere in `GUARD_COMMAND`, the last one winning, so in
`S=<dir>; git commit -m "note S=/nonexistent"; node $S/x.cjs` the path becomes `/nonexistent/x.cjs`,
which does not exist, and a raw-playwright `x.cjs` runs unscanned.

## What unblocks it

The approve-guard half is done: `guard.mjs` tokenises the command and matches only argv positions that
would execute.

In `block-raw-playwright.mjs`, test the `ALLOWED_PATH_RE` allowlist and the inline `RAW_PLAYWRIGHT_RE`
(lines 35 and 42) against the matched segment's `GUARD_ARG_*` tokens instead of `GUARD_COMMAND`. Its
file scan already takes script paths only from those tokens, but it expands `$VAR` in them from `VAR=`
assignments found anywhere in `GUARD_COMMAND`, quoted text included (lines 52-57); narrowing it must keep
an earlier segment's assignment (`S=/tmp/x; node $S/shot.cjs`) without trusting a quoted one. A segment's tokens leave out heredoc bodies, here-strings
and redirect targets, so the narrowed inline test must still read what the `node` segment takes on
stdin: today `node <<EOF` with a raw-playwright body is refused only because the whole string is
scanned. For the file scan, match real `require`/`import` statements (AST) or exempt `node --check`.

`block-secret-exposure.mjs` keeps `GUARD_COMMAND`. Its header says it reads the whole command so it
sees the whole picture whichever segment matched, and a segment never shows a redirect or the other
side of a pipe: `wf secret get KEY > /tmp/k` tokenises to just `wf secret get KEY`. Scanning only
`GUARD_ARG_*` would let through both leaks that `libraries/hook-runtime/tests/guard.test.ts` expects to
be denied. Instead, its own quote-aware parser should skip heredoc bodies and comments, as
`splitCommands` already does.

Each change needs a test for "dangerous command disguised as quoted text".

## Re-checked 2026-10-20

Re-ran every case in the gap through guard.mjs's decide() with the real deny.json (probe at /tmp/claude-0/gaps/probe/probe.mjs), and all of them reproduce. Quoted, heredoc and comment text still trigger the raw-playwright and secret-exposure conditions. A trailing allowlist mention, stdin-fed node and a quoted `S=` assignment still let a raw-playwright file run. block-raw-playwright.mjs:35 and :42 still test the whole GUARD_COMMAND; its only change since 4 Oct (dcab33aae) is a comment. One addition, fixed the same day: ALLOWED_PATH_RE (line 33) also allowlisted a library deleted earlier, a second string that exempted a real raw-playwright run; it now names only `libraries/browser-client`.
