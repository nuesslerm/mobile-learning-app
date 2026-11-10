#!/usr/bin/env node
import { spawn } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { decide, denyOutput } from './guard.mjs';
import { failLoud, readHookInput } from './index.mjs';

const HOOK_NAME = 'agent-hook';

/** Every agent hook, per event. Steps of one event run in parallel, as sibling hooks did. */
export const STEPS = {
  SessionStart: [
    { run: 'wf agent-config sync' },
    { run: 'bash .agents/hooks/session-setup.sh' },
  ],
  UserPromptSubmit: [
    { run: 'bash .agents/hooks/session-heal.sh' },
  ],
  PreToolUse: [
    { run: 'wf agent-config mcp-guard', tools: /^mcp__/ },
    { run: 'bash .agents/hooks/rtk-rewrite.sh', tools: /^Bash$/ },
  ],
  PostToolUse: [{ run: 'node scripts/hooks/mask-large-output.mjs', tools: /^(Bash|Read)$/ }],
};

const CONTEXT_EVENTS = new Set(['SessionStart', 'UserPromptSubmit', 'PreToolUse', 'PostToolUse']);

export function stepsFor(payload) {
  return (STEPS[payload.hook_event_name] ?? []).filter(
    (s) => !s.tools || s.tools.test(payload.tool_name ?? '')
  );
}

function runStep(step, input, root) {
  return new Promise((done) => {
    const child = spawn('sh', ['-c', step.run], { cwd: root, env: { ...process.env, CLAUDE_PROJECT_DIR: root } });
    const out = { run: step.run, status: null, stdout: '', stderr: '' };
    child.stdout.on('data', (d) => (out.stdout += d));
    child.stderr.on('data', (d) => (out.stderr += d));
    child.on('error', (e) => (out.stderr += String(e)));
    child.on('close', (code) => done({ ...out, status: code }));
    child.stdin.on('error', () => {});
    child.stdin.end(input);
  });
}

/** Folds sibling results into one hook reply: any exit 2 blocks, JSON replies merge, plain text becomes context. */
export function merge(event, results) {
  const blocked = results.filter((r) => r.status === 2);
  if (blocked.length > 0) return { code: 2, stdout: '', stderr: blocked.map((r) => r.stderr.trim()).join('\n') };
  const failed = results.filter((r) => r.status !== 0);
  const stderr = results.map((r) => r.stderr).join('') + failed.map((r) => `${HOOK_NAME}: "${r.run}" exited ${r.status}\n`).join('');
  const texts = [];
  const contexts = [];
  let top = {};
  let specific = {};
  for (const r of results.filter((x) => x.status === 0)) {
    const out = r.stdout.trim();
    if (!out) continue;
    let json;
    try {
      json = JSON.parse(out);
    } catch {
      json = null;
    }
    if (!json || typeof json !== 'object') {
      texts.push(out);
      continue;
    }
    const { hookSpecificOutput, ...rest } = json;
    top = { ...top, ...rest };
    const { additionalContext, ...fields } = hookSpecificOutput ?? {};
    if (additionalContext) contexts.push(additionalContext);
    specific = { ...specific, ...fields };
  }
  const structured = Object.keys(top).length + Object.keys(specific).length + contexts.length > 0;
  if (!structured && (failed.length === 0 || texts.length === 0)) {
    return { code: failed.length > 0 ? 1 : 0, stdout: texts.join('\n'), stderr };
  }
  const context = [...texts, ...contexts].join('\n\n');
  if (context && CONTEXT_EVENTS.has(event)) specific.additionalContext = context;
  if (Object.keys(specific).length > 0) top.hookSpecificOutput = { hookEventName: event, ...specific };
  if (failed.length > 0) top.systemMessage = failed.map((r) => `hook "${r.run}" failed (exit ${r.status})`).join('; ');
  return { code: 0, stdout: JSON.stringify(top), stderr };
}

export async function dispatch(payload, { root, guardOnly = false, run = runStep } = {}) {
  if (payload.hook_event_name === 'PreToolUse') {
    const reason = decide(payload, { root });
    if (reason) return { code: 0, stdout: denyOutput(reason), stderr: '' };
    if (guardOnly) return { code: 0, stdout: '', stderr: '' };
  }
  const input = JSON.stringify(payload);
  const results = await Promise.all(stepsFor(payload).map((s) => run(s, input, root)));
  return merge(payload.hook_event_name, results);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  const root = process.env.CLAUDE_PROJECT_DIR ?? resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
  const payload = readHookInput(HOOK_NAME);
  const guardOnly = process.argv[2] === 'guard';
  if (guardOnly) payload.hook_event_name = 'PreToolUse';
  if (typeof payload.hook_event_name !== 'string') failLoud(HOOK_NAME, 'payload has no hook_event_name');
  try {
    const result = await dispatch(payload, { root, guardOnly });
    if (result.stdout) process.stdout.write(`${result.stdout}\n`);
    if (result.stderr) process.stderr.write(result.stderr);
    process.exitCode = result.code;
  } catch (error) {
    failLoud(HOOK_NAME, `${payload.hook_event_name} dispatch threw`, error);
  }
}
