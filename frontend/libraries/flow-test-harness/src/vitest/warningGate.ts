import { format } from 'node:util';

type WarningConsole = Pick<Console, 'warn' | 'error'>;

type Gate = { check: () => void; expect: (pattern: RegExp) => void };

let active: Gate | undefined;

/**
 * A warning fails the flow test that caused it, as it fails a device harness run: the console's warn and error (what the
 * flow report lists as warnings) are kept as they print, and `check` throws with every kept line, then forgets them. A
 * warning the test causes on purpose is named first with `expect`, and then it must print (and is not reported).
 */
export function installWarningGate(target: WarningConsole = console): Gate {
  const kept: string[] = [];
  let expected: RegExp[] = [];
  for (const level of ['warn', 'error'] as const) {
    const original = target[level].bind(target);
    target[level] = (...args: unknown[]) => {
      const line = format(...args);
      kept.push(line);
      // a warning the test named in advance is part of what it asserts: it stays out of the report's warning count
      if (!expected.some((p) => p.test(line))) original(...args);
    };
  }
  const gate: Gate = {
    expect(pattern) {
      expected.push(pattern);
    },
    check() {
      const lines = kept.splice(0);
      const unseen = expected.filter((p) => !lines.some((line) => p.test(line)));
      const stray = lines.filter((line) => !expected.some((p) => p.test(line)));
      expected = [];
      const problems = [
        ...stray.map((line) => `  - ${line.split('\n')[0]}`),
        ...unseen.map((p) => `  - expected a warning matching ${p} and none printed`),
      ];
      if (problems.length === 0) return;
      throw new Error(`${problems.length} warning(s); a flow test passes with none:\n${problems.join('\n')}`);
    },
  };
  active = gate;
  return gate;
}

/** Names a warning this test causes on purpose (a crash it logs, say): it passes the gate, and the test fails without it. */
export function expectWarning(pattern: RegExp): void {
  if (!active) throw new Error('expectWarning: no warning gate is installed (the flow setup installs it)');
  active.expect(pattern);
}
