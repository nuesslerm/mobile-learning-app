import { spawnSync } from 'node:child_process';
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

import { afterAll, beforeAll, describe, expect, it } from 'vitest';

const HOOK = resolve(import.meta.dirname, '../../../.agents/hooks/rtk-rewrite.sh');

// A stand-in for `rtk rewrite`: cargo has a rule that asks (exit 3), ls and grep a plain rewrite (exit 0), the rest and anything
// piped or redirected none (exit 1), which is how the real registry answers. The hook only needs the exit code and the text.
const STUB = `#!/bin/sh
[ "$1" = "--version" ] && { echo "rtk 0.47.0"; exit 0; }
[ "$1" = "rewrite" ] || exit 1
case "$2" in
  *2\\>\\&1*|*\\|*) exit 1 ;;
  cargo*|"cd "*"&& cargo"*) printf '%s' "$(printf '%s' "$2" | sed 's/cargo/rtk cargo/')"; exit 3 ;;
  ls*|grep*) printf 'rtk %s' "$2"; exit 0 ;;
  *) exit 1 ;;
esac
`;

let bin: string;
let cache: string;

beforeAll(() => {
  bin = mkdtempSync(join(tmpdir(), 'rtk-stub-'));
  cache = mkdtempSync(join(tmpdir(), 'rtk-cache-'));
  writeFileSync(join(bin, 'rtk'), STUB);
  chmodSync(join(bin, 'rtk'), 0o755);
});

afterAll(() => {
  rmSync(bin, { recursive: true, force: true });
  rmSync(cache, { recursive: true, force: true });
});

const run = (command: string) =>
  spawnSync('bash', [HOOK], {
    input: JSON.stringify({ tool_input: { command } }),
    encoding: 'utf8',
    env: { ...process.env, PATH: `${bin}:${process.env['PATH']}`, XDG_CACHE_HOME: cache },
  });

describe('given a pipeline that only trims the output of a command that has an rtk form', () => {
  it.each([
    ['a cargo test with the stderr merged', 'cargo test --release -p x 2>&1 | tail -30', 'rtk cargo test --release -p x'],
    ['a cargo build after a cd', 'cd backend/x && cargo build | head -60', 'cd backend/x && rtk cargo build'],
    ['a plain cargo test', 'cargo test | tail', 'rtk cargo test'],
    ['an ls', 'ls -la src | head', 'rtk ls -la src'],
    ['a grep', 'grep -n foo src | head -20', 'rtk grep -n foo src'],
  ])('when %s is run, then it is blocked and the rtk form is named', (_name, command, suggestion) => {
    const result = run(command);

    expect(result.status).toBe(2);
    expect(result.stderr).toContain(`Run it as: ${suggestion}`);
  });
});

describe('given a pipeline that is not only a trim', () => {
  it.each([
    ['a command that has no rtk form', 'python3 script.py | tail -5'],
    ['a pipe into a consumer of the bytes', 'cargo test | grep -c ok'],
    ['an explicit rtk command', 'rtk cargo test | tail -5'],
    ['a quoted head, which cannot be split safely', 'cargo test --features "a b" | tail'],
  ])('when %s is run, then it passes through untouched', (_name, command) => {
    const result = run(command);

    expect(result.status).toBe(0);
    expect(result.stdout.trim()).toBe('');
  });
});

describe('given a command without a pipe', () => {
  it('when a cargo build is run, then it is rewritten to rtk and left for the user to confirm', () => {
    const result = run('cargo build --release');

    expect(result.status).toBe(0);
    const reply = JSON.parse(result.stdout) as { hookSpecificOutput: { updatedInput: { command: string }; permissionDecision?: string } };
    expect(reply.hookSpecificOutput.updatedInput.command).toBe('rtk cargo build --release');
    expect(reply.hookSpecificOutput.permissionDecision).toBeUndefined();
  });
});
