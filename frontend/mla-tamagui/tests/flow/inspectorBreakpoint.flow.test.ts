import { breakAt } from '@lib/flow-test-harness';
import { Alert } from 'react-native';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { installDeviceLab, type DeviceLab } from '@/lib/device-lab';

type LabScope = typeof globalThis & { __deviceLab?: DeviceLab };

describe('a flow test stopped at a line', () => {
  afterEach(() => {
    delete (globalThis as LabScope).__deviceLab;
    document.body.innerHTML = '';
  });

  it('Given a breakpoint where the lab Alert runs a button, When Delete is tapped, Then the pause reads the pressed index and the flow goes on', () => {
    // Given the lab Alert showing a confirm, with a breakpoint on the line that runs the pressed button
    installDeviceLab();
    const onDelete = vi.fn();
    Alert.alert('Delete draft?', undefined, [{ text: 'Cancel', style: 'cancel' }, { text: 'Delete', onPress: onDelete }]);
    const bp = breakAt('lib/device-lab/alert.ts', 25, ['index', 'pending.buttons[index].text']);

    // When the person taps Delete
    try {
      document.querySelector<HTMLElement>('[data-testid="LabAlert-Button-1"]')?.click();
    } finally {
      bp.release();
    }

    // Then the test stopped once at that line, read the locals and the stack there, and resumed into the handler
    expect(bp.stops).toEqual([
      expect.objectContaining({ kind: 'breakpoint', file: 'lib/device-lab/alert.ts', line: 25, values: { index: 1, 'pending.buttons[index].text': 'Delete' } }),
    ]);
    expect(bp.stops[0].stack[0]).toEqual({ function: bp.stops[0].function, file: 'lib/device-lab/alert.ts', line: 25 });
    expect(bp.stops[0].stack.some((f) => f.file === 'tests/flow/inspectorBreakpoint.flow.test.ts')).toBe(true);
    expect(onDelete).toHaveBeenCalledTimes(1);
  });
});
