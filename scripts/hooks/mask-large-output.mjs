#!/usr/bin/env node
// PostToolUse hook, the `mask-reads` token-diet lever.
//
// The shipped `scripts/hooks/mask-large-output.mjs` only APPENDS a one-line
// pointer after a big Bash result; the full result still enters context and is
// re-read on every later call (the 81% cache-read share in the token map). This
// variant uses `updatedToolOutput`, which REPLACES what the model sees, so a big
// Bash or Read result enters context as its head plus a pointer to the full
// text on disk. The model can Read the file with offset/limit when it needs
// more. Measured as an arm before it replaces the shipped hook.
//
// Env: MASK_THRESHOLD (bytes, default 800), MASK_HEAD_LINES (default 40).

import { createHash } from 'node:crypto';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import {
  failLoud,
  readHookInput,
} from '../../libraries/hook-runtime/src/index.mjs';

const HOOK_NAME = 'mask-tool-output';
const THRESHOLD = Number(process.env.MASK_THRESHOLD ?? '800');
const HEAD_LINES = Number(process.env.MASK_HEAD_LINES ?? '40');

const projectDir = process.env.CLAUDE_PROJECT_DIR ?? process.cwd();
const tmpDir = join(projectDir, '.agents', 'tmp', 'masked');

const input = readHookInput(HOOK_NAME);
if (!input || typeof input !== 'object') process.exit(0);

const tool = input.tool_name;
const response = input.tool_response;
if (!response || typeof response !== 'object') process.exit(0);

/** Where the big text lives for this tool, and how to rebuild the response. */
function pick() {
  if (tool === 'Bash' && typeof response.stdout === 'string') {
    return {
      text: response.stdout,
      kind: 'bash',
      rebuild: (head) => ({ ...response, stdout: head }),
    };
  }
  if (tool === 'Read' && typeof response.file_contents === 'string') {
    return {
      text: response.file_contents,
      kind: 'read',
      rebuild: (head) => ({ ...response, file_contents: head }),
    };
  }
  return null;
}

const picked = pick();
if (!picked || picked.text.length < THRESHOLD) process.exit(0);

const { text, kind, rebuild } = picked;
const id = createHash('sha256').update(text).digest('hex').slice(0, 8);
const relPath = `.agents/tmp/masked/${kind}-${id}.txt`;
try {
  mkdirSync(tmpDir, { recursive: true });
  writeFileSync(join(projectDir, relPath), text, 'utf-8');
} catch (error) {
  failLoud(HOOK_NAME, `could not write the offload file ${relPath}`, error);
}

const lines = text.split('\n');
const head = lines.slice(0, HEAD_LINES).join('\n');
const trailer =
  `\n[${HOOK_NAME}: ${lines.length} lines / ${text.length} bytes; first ${Math.min(HEAD_LINES, lines.length)} shown. ` +
  `Full text at ${relPath}; Read it with offset/limit for a specific section, never whole.]`;

process.stdout.write(
  JSON.stringify({
    hookSpecificOutput: {
      hookEventName: 'PostToolUse',
      updatedToolOutput: rebuild(head + trailer),
    },
  })
);
