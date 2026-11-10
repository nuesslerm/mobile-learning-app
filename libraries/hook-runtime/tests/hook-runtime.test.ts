import { describe, it, expect } from 'vitest';
import { execFileSync } from 'node:child_process';
import { join } from 'node:path';

/**
 * Driven as a real subprocess, not by importing the functions: the whole
 * contract here IS the process exit code and stderr, and a unit test that
 * stubbed `process.exit` would pass while the deployed behaviour regressed.
 * That is the same "tested a copy, not the thing" trap this repo has hit
 * before.
 */
const RUNTIME = join(import.meta.dirname, '../src/index.mjs');

function runScript(
  body: string,
  stdin = ''
): { code: number; stdout: string; stderr: string } {
  const script = `import { HOOK_EXIT, failLoud, proceed, block, readHookInput, runHook } from ${JSON.stringify(RUNTIME)};\n${body}`;
  try {
    const stdout = execFileSync(process.execPath, ['--input-type=module', '-e', script], {
      input: stdin,
      encoding: 'utf-8',
      stdio: ['pipe', 'pipe', 'pipe'],
    });
    return { code: 0, stdout, stderr: '' };
  } catch (err) {
    const e = err as { status?: number; stdout?: string; stderr?: string };
    return {
      code: e.status ?? -1,
      stdout: e.stdout ?? '',
      stderr: e.stderr ?? '',
    };
  }
}

describe('given a hook that cannot do its job', () => {
  it('when failLoud is called, then it exits 1 and names the hook on stderr', () => {
    const r = runScript(`failLoud('my-hook', 'could not read config', new Error('ENOENT'));`);
    expect(r.code).toBe(1);
    expect(r.stderr).toContain('my-hook NOT ENFORCING — could not read config');
    expect(r.stderr).toContain('ENOENT');
  });

  it('when failLoud is called with no error, then it still names the failure', () => {
    const r = runScript(`failLoud('my-hook', 'config was empty');`);
    expect(r.code).toBe(1);
    expect(r.stderr).toContain('my-hook NOT ENFORCING — config was empty');
  });
});

// The bug this module exists to make unwritable: an internal error that
// silently becomes a successful, silent exit 0.
describe('given a hook body that throws', () => {
  it('when wrapped in runHook, then it exits 1 rather than 0', () => {
    const r = runScript(
      `await runHook('my-hook', () => { throw new Error('boom'); });`
    );
    expect(r.code).toBe(1);
    expect(r.stderr).toContain('my-hook NOT ENFORCING — threw while running');
    expect(r.stderr).toContain('boom');
  });

  it('when the body rejects asynchronously, then it also exits 1', () => {
    const r = runScript(
      `await runHook('my-hook', async () => { throw new Error('async boom'); });`
    );
    expect(r.code).toBe(1);
    expect(r.stderr).toContain('async boom');
  });

  it('when the body succeeds, then it exits 0 silently', () => {
    const r = runScript(`await runHook('my-hook', () => {});`);
    expect(r.code).toBe(0);
    expect(r.stderr).toBe('');
  });
});

describe('given hook input on stdin', () => {
  it('when it is valid JSON, then it is returned parsed', () => {
    const r = runScript(
      `const i = readHookInput('my-hook'); console.log(i.tool_name);`,
      '{"tool_name":"Bash"}'
    );
    expect(r.code).toBe(0);
    expect(r.stdout.trim()).toBe('Bash');
  });

  it('when it is not JSON, then it exits 1 rather than proceeding', () => {
    const r = runScript(`readHookInput('my-hook');`, 'not-json{{{');
    expect(r.code).toBe(1);
    expect(r.stderr).toContain('could not parse hook input as JSON');
  });

  it('when it is valid JSON but not an object, then it exits 1', () => {
    const r = runScript(`readHookInput('my-hook');`, '"just a string"');
    expect(r.code).toBe(1);
    expect(r.stderr).toContain('was not a JSON object');
  });
});

describe('given the two intentional outcomes', () => {
  it('when proceed is called, then it exits 0 with no output', () => {
    const r = runScript(`proceed();`);
    expect(r.code).toBe(0);
    expect(r.stderr).toBe('');
  });

  it('when block is called, then it exits 2 with the reason on stderr', () => {
    const r = runScript(`block('BLOCKED: do the other thing instead');`);
    expect(r.code).toBe(2);
    expect(r.stderr).toContain('BLOCKED: do the other thing instead');
  });

  it('when the exit codes are read, then they match the documented contract', () => {
    const r = runScript(`console.log(JSON.stringify(HOOK_EXIT));`);
    expect(JSON.parse(r.stdout)).toEqual({
      PROCEED: 0,
      NOT_ENFORCING: 1,
      BLOCK: 2,
    });
  });
});
