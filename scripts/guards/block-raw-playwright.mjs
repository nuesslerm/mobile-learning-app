#!/usr/bin/env node
// `condition` script for `libraries/hook-runtime/src/deny.json` -- blocks a `node`
// invocation that reaches for the raw `playwright`/`playwright-core`
// package directly, instead of driving a real browser through the `wf
// browser` CLI (the Rust daemon in `clis/wf-harness`, reachable from node scripts through
// `@lib/browser-client`). This is exactly the mistake made repeatedly
// in one real session: ad-hoc scratch scripts under /tmp hand-launched
// Chromium via `require('playwright-core')` reached off the repo's own
// shamefully-hoisted root node_modules (see CLAUDE.md's "browser verification
// is the wf browser CLI" line, correct and ignored). A source lint can never see a /tmp scratch
// file; this guard catches it at the point the command actually runs.
//
// Two detection paths, since the violating pattern took both shapes in that
// session: (1) an inline `node -e '...require("playwright-core")...'`, where
// GUARD_COMMAND already contains the whole script; (2) `node /tmp/foo.js`,
// where the reference lives in a separate file GUARD_COMMAND only names --
// so any `.js`/`.mjs`/`.cjs`/`.ts` path token in the command is also read
// and scanned, best-effort (an unreadable/unresolvable path is skipped, not
// a false block).
//
// Allowlisted: libraries/browser-client (the daemon client). Zero-dependency so it runs before any build step
// completes on the PreToolUse critical path.
//
// Receives the full original command string via GUARD_COMMAND (see
// libraries/hook-runtime/src/guard.mjs runCondition).

import { readFileSync, existsSync } from 'node:fs';
import { resolve } from 'node:path';

const command = process.env.GUARD_COMMAND ?? '';

const ALLOWED_PATH_RE = /\blibraries\/browser-client\b/;

if (ALLOWED_PATH_RE.test(command)) {
  process.exit(1);
}

const RAW_PLAYWRIGHT_RE =
  /require\(\s*['"]playwright(-core)?['"]\s*\)|from\s+['"]playwright(-core)?['"]|import\(\s*['"]playwright(-core)?['"]\s*\)|node_modules\/playwright(-core)?\b|\b(chromium|firefox|webkit)\.launch\s*\(/;

if (RAW_PLAYWRIGHT_RE.test(command)) {
  process.exit(0);
}

// Second path: a referenced script file, not the inline command itself.
// Skips scripts/guards/ -- this file (and any sibling guard script) legitimately
// names these patterns as regex source text, not usage, and would otherwise
// false-positive on itself the moment a command references its own path.
// Script paths are often written through a shell variable assigned earlier in
// the same command (`S=/tmp/x; node $S/shot.cjs`); expand those before reading.
const assigned = {};
const ASSIGN_RE = /(?:^|[\s;&|(])([A-Za-z_][A-Za-z0-9_]*)=("[^"]*"|'[^']*'|[^\s;&|)]+)/g;
let assignment;
while ((assignment = ASSIGN_RE.exec(command)) !== null) {
  assigned[assignment[1]] = assignment[2].replace(/^["']|["']$/g, '');
}
const expandVars = (token) =>
  token.replace(/\$\{?([A-Za-z_][A-Za-z0-9_]*)\}?/g, (whole, name) => assigned[name] ?? process.env[name] ?? whole);

// Only the matched `node` command's own arguments name scripts it will run; other segments of a compound command do not.
const segmentArgs = Object.keys(process.env)
  .filter((k) => /^GUARD_ARG_\d+$/.test(k))
  .sort((a, b) => Number(a.slice(10)) - Number(b.slice(10)))
  .map((k) => process.env[k]);
const scanned = segmentArgs.length > 0 ? segmentArgs.join(' ') : command;
const FILE_TOKEN_RE = /(?:^|[\s'"])([^\s'"]+\.(?:m?js|cjs|ts))(?=[\s'"]|$)/g;
let match;
while ((match = FILE_TOKEN_RE.exec(scanned)) !== null) {
  const candidate = expandVars(match[1]);
  if (candidate.includes('scripts/guards/')) continue;
  // Fail closed: a script path the guard cannot resolve is one it cannot read.
  if (/\$\{?[A-Za-z_]/.test(candidate)) process.exit(0);
  const resolved = candidate.startsWith('/') ? candidate : resolve(process.cwd(), candidate);
  if (!existsSync(resolved)) continue;
  try {
    const content = readFileSync(resolved, 'utf8');
    if (RAW_PLAYWRIGHT_RE.test(content)) {
      process.exit(0);
    }
  } catch {
    // unreadable — skip, this is a best-effort second layer
  }
}

process.exit(1);
