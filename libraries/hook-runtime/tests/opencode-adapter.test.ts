import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import {
  createOpencodeHooksPlugin,
  matcherMatches,
  normalizeToolName,
  runHooksFor,
  toToolInput,
} from '../src/opencode-adapter.mjs';

describe('given matcherMatches / normalizeToolName', () => {
  it('when opencode reports lowercase tool names, then Claude-style matchers still match exactly (anchored, case-insensitive)', () => {
    expect(normalizeToolName('bash')).toBe('Bash');
    expect(normalizeToolName('multi-browser_session_open')).toBe(
      'multi-browser_session_open'
    );
    expect(matcherMatches('Bash', 'Bash')).toBe(true);
    expect(matcherMatches('Bash', 'BashOutput')).toBe(false);
    expect(matcherMatches('Edit|Write', 'Write')).toBe(true);
    expect(matcherMatches(undefined, 'anything')).toBe(true);
  });
});

describe('given toToolInput', () => {
  it('when opencode args use camelCase, then Claude Code tool_input keys are added alongside', () => {
    expect(toToolInput('Bash', { command: 'ls' })).toEqual({ command: 'ls' });
    expect(
      toToolInput('Edit', { filePath: '/a', oldString: 'x', newString: 'y' })
    ).toMatchObject({ file_path: '/a', old_string: 'x', new_string: 'y' });
  });
});

describe('given runHooksFor with real shell commands', () => {
  let dir: string;
  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), 'opencode-adapter-'));
  });
  afterEach(() => rmSync(dir, { recursive: true, force: true }));

  it('when a hook matches, then it receives the Claude-shaped payload on stdin and CLAUDE_PROJECT_DIR in its env', () => {
    const capture = join(dir, 'seen.json');
    const hooks = {
      'tool.execute.before': [
        {
          matcher: 'Bash',
          hooks: [
            {
              type: 'command',
              command: `cat > "${capture}"; echo "$CLAUDE_PROJECT_DIR" > "${capture}.cwd"`,
            },
          ],
        },
      ],
    };

    runHooksFor({
      hooks,
      event: 'tool.execute.before',
      toolName: 'Bash',
      payload: { tool_name: 'Bash', tool_input: { command: 'uuidgen' } },
      cwd: dir,
    });

    expect(JSON.parse(readFileSync(capture, 'utf8'))).toEqual({
      tool_name: 'Bash',
      tool_input: { command: 'uuidgen' },
    });
    expect(readFileSync(`${capture}.cwd`, 'utf8').trim()).toBe(dir);
  });

  it('when a hook exits 2, then the adapter throws with its stderr (a block)', () => {
    const hooks = {
      'session.deleted': [
        {
          hooks: [
            {
              type: 'command',
              command: 'echo "unfinished: fix it" >&2; exit 2',
            },
          ],
        },
      ],
    };

    expect(() =>
      runHooksFor({ hooks, event: 'session.deleted', payload: {}, cwd: dir })
    ).toThrow('unfinished: fix it');
  });

  it('when a hook exits 0 with a deny decision on stdout, then the adapter throws with the reason', () => {
    const deny = JSON.stringify({
      hookSpecificOutput: {
        hookEventName: 'PreToolUse',
        permissionDecision: 'deny',
        permissionDecisionReason: 'APPROVAL REQUIRED',
      },
    });
    const hooks = {
      'tool.execute.before': [
        {
          matcher: 'Bash',
          hooks: [{ type: 'command', command: `echo '${deny}'` }],
        },
      ],
    };

    expect(() =>
      runHooksFor({
        hooks,
        event: 'tool.execute.before',
        toolName: 'Bash',
        payload: {},
        cwd: dir,
      })
    ).toThrow('APPROVAL REQUIRED');
  });

  it('when a hook exits 1, then it is logged loudly but never blocks', () => {
    const logged: string[] = [];
    const hooks = {
      'tool.execute.after': [
        { hooks: [{ type: 'command', command: 'echo boom >&2; exit 1' }] },
      ],
    };

    const results = runHooksFor({
      hooks,
      event: 'tool.execute.after',
      toolName: 'Bash',
      payload: {},
      cwd: dir,
      log: (m: string) => logged.push(m),
    });

    expect(results[0]!.exitCode).toBe(1);
    expect(logged.join('\n')).toContain('boom');
  });

  it('when the matcher does not cover the tool, then nothing runs', () => {
    const hooks = {
      'tool.execute.before': [
        { matcher: 'Bash', hooks: [{ type: 'command', command: 'exit 2' }] },
      ],
    };

    expect(
      runHooksFor({
        hooks,
        event: 'tool.execute.before',
        toolName: 'Read',
        payload: {},
        cwd: dir,
      })
    ).toEqual([]);
  });
});

describe('given createOpencodeHooksPlugin over a hooks.json file', () => {
  let dir: string;
  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), 'opencode-plugin-'));
  });
  afterEach(() => rmSync(dir, { recursive: true, force: true }));

  it('when opencode fires tool.execute.before for bash, then the Bash hook gets tool_input.command and a deny becomes a thrown error', async () => {
    const seen = join(dir, 'seen.json');
    const deny = JSON.stringify({
      hookSpecificOutput: {
        hookEventName: 'PreToolUse',
        permissionDecision: 'deny',
        permissionDecisionReason: 'BLOCKED by guard',
      },
    });
    writeFileSync(
      join(dir, 'hooks.json'),
      JSON.stringify({
        hooks: {
          'tool.execute.before': [
            {
              matcher: 'Bash',
              hooks: [
                {
                  type: 'command',
                  command: `tee "${seen}" >/dev/null; echo '${deny}'`,
                },
              ],
            },
          ],
        },
      })
    );
    const plugin = createOpencodeHooksPlugin({
      hooksFile: join(dir, 'hooks.json'),
    });
    const api = await plugin({ directory: dir });

    await expect(
      api['tool.execute.before'](
        { tool: 'bash', sessionID: 's1', callID: 'c1' },
        { args: { command: 'uuidgen' } }
      )
    ).rejects.toThrow('BLOCKED by guard');
    const payload = JSON.parse(readFileSync(seen, 'utf8'));
    expect(payload).toMatchObject({
      hook_event_name: 'PreToolUse',
      tool_name: 'Bash',
      tool_input: { command: 'uuidgen' },
      session_id: 's1',
      cwd: dir,
    });
  });

  it('when the hooks file is missing, then the plugin loads and every event is a no-op', async () => {
    const plugin = createOpencodeHooksPlugin({
      hooksFile: join(dir, 'nope.json'),
    });
    const api = await plugin({ directory: dir });
    await expect(
      api['session.deleted']({ sessionID: 's' })
    ).resolves.toBeUndefined();
  });
});
