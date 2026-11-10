import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { dispatch, merge, STEPS, stepsFor } from '../src/dispatch.mjs';

const repoRoot = resolve(import.meta.dirname, '../../..');
const DISPATCH = resolve(import.meta.dirname, '../src/dispatch.mjs');
const ok = (stdout = '', stderr = '') => ({ run: 'x', status: 0, stdout, stderr });

describe('given the dispatcher step table', () => {
  it('when every step names a script, then that script exists in the repo', () => {
    const scripts = Object.values(STEPS)
      .flat()
      .flatMap((s: { run: string }) => s.run.split(' ').filter((w) => /\.(mjs|js|sh)$/.test(w)));
    expect(scripts.filter((p) => !existsSync(resolve(repoRoot, p)))).toEqual([]);
  });
});

describe('given a PreToolUse payload', () => {
  it('when the tool is Bash, then only the rtk rewrite runs after the guard', () => {
    const steps = stepsFor({ hook_event_name: 'PreToolUse', tool_name: 'Bash' });
    expect(steps.map((s: { run: string }) => s.run)).toEqual(['bash .agents/hooks/rtk-rewrite.sh']);
  });

  it('when the guard denies, then no sibling step runs', async () => {
    let ran = 0;
    const result = await dispatch(
      { hook_event_name: 'PreToolUse', tool_name: 'Bash', tool_input: { command: 'git branch -D x' } },
      { root: repoRoot, run: async () => (ran++, ok()) }
    );
    expect(ran).toBe(0);
    expect(JSON.parse(result.stdout).hookSpecificOutput.permissionDecision).toBe('deny');
  });

  it('when run as a process in guard mode, then a denied command exits 0 with a deny decision', () => {
    const stdout = execFileSync(process.execPath, [DISPATCH, 'guard'], {
      input: JSON.stringify({ tool_name: 'Bash', tool_input: { command: 'git reset --hard' } }),
      encoding: 'utf8',
      env: { ...process.env, CLAUDE_PROJECT_DIR: repoRoot },
    });
    expect(JSON.parse(stdout).hookSpecificOutput.permissionDecision).toBe('deny');
  });
});

describe('given sibling hook results', () => {
  it('when one exits 2, then the merged result blocks with its stderr', () => {
    const r = merge('Stop', [ok('fine'), { run: 'y', status: 2, stdout: '', stderr: 'publish first\n' }]);
    expect(r).toMatchObject({ code: 2, stderr: 'publish first' });
  });

  it('when plain text and an additionalContext reply mix, then both reach the context of one JSON reply', () => {
    const reply = JSON.stringify({ hookSpecificOutput: { hookEventName: 'UserPromptSubmit', additionalContext: 'B' } });
    const r = merge('UserPromptSubmit', [ok('A'), ok(reply)]);
    expect(JSON.parse(r.stdout).hookSpecificOutput).toEqual({ hookEventName: 'UserPromptSubmit', additionalContext: 'A\n\nB' });
  });

  it('when one fails and nothing else spoke, then the merged result exits 1 and names it', () => {
    const r = merge('SessionStart', [ok(), { run: 'bash setup.sh', status: 1, stdout: '', stderr: 'boom\n' }]);
    expect(r.code).toBe(1);
    expect(r.stderr).toContain('"bash setup.sh" exited 1');
  });

  it('when one fails but another produced context, then the context survives with a systemMessage', () => {
    const r = merge('SessionStart', [ok('watching'), { run: 'bash setup.sh', status: 1, stdout: '', stderr: '' }]);
    expect(r.code).toBe(0);
    expect(JSON.parse(r.stdout)).toMatchObject({
      systemMessage: 'hook "bash setup.sh" failed (exit 1)',
      hookSpecificOutput: { additionalContext: 'watching' },
    });
  });
});
