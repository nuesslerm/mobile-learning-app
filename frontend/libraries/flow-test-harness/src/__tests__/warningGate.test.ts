import { describe, expect, it } from 'vitest';

import { expectWarning, installWarningGate } from '../vitest/warningGate';

function quietConsole() {
  const printed: string[] = [];
  return {
    printed,
    target: {
      warn: (...a: unknown[]) => printed.push(a.join(' ')),
      error: (...a: unknown[]) => printed.push(a.join(' ')),
    },
  };
}

describe('the flow warning gate', () => {
  it('Given a test that prints a warning and an error, When the gate checks, Then it throws naming both, and they still print', () => {
    // Given a console behind the gate
    const { printed, target } = quietConsole();
    const gate = installWarningGate(target);

    // When a test warns and errors
    target.warn('Animated: `useNativeDriver` is not supported');
    target.error(
      'Received `%s` for a non-boolean attribute `%s`.',
      true,
      'selectable\nmore detail'
    );

    // Then the check fails with both first lines, and the report still sees them printed
    expect(() => gate.check()).toThrow(
      '2 warning(s); a flow test passes with none:\n  - Animated: `useNativeDriver` is not supported\n  - Received `true` for a non-boolean attribute `selectable'
    );
    expect(printed).toHaveLength(2);
  });

  it('Given a test that printed nothing, or a gate already checked, When the gate checks, Then it passes', () => {
    // Given a gate that already reported one warning
    const { target } = quietConsole();
    const gate = installWarningGate(target);
    expect(gate.check).not.toThrow();
    target.warn('once');
    expect(gate.check).toThrow('1 warning(s)');

    // When the next test prints nothing, Then its check passes
    expect(gate.check).not.toThrow();
  });

  it('Given a warning the test names first, When the gate checks, Then that warning passes and a named one that never printed fails', () => {
    // Given a gate and a warning the test expects
    const { printed, target } = quietConsole();
    const gate = installWarningGate(target);
    expectWarning(/app\.crash/);

    // When it prints, Then the check passes and the line stays out of the console the report reads
    target.error('app.crash', { kind: 'error' });
    expect(gate.check).not.toThrow();
    expect(printed).toHaveLength(0);

    // When a test names one that never prints, Then the check fails saying so
    gate.expect(/app\.crash/);
    expect(gate.check).toThrow('expected a warning matching /app\\.crash/ and none printed');
  });
});
