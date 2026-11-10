import { afterEach, describe, expect, it } from 'vitest';

import { DEVICE_LAB_SENTINEL, installDeviceLab } from '../index';

type LabScope = typeof globalThis & { __deviceLab?: unknown };

describe('installDeviceLab', () => {
  afterEach(() => {
    delete (globalThis as LabScope).__deviceLab;
  });

  it('publishes an empty lab under the sentinel version', () => {
    // Given a page with no lab installed
    expect((globalThis as LabScope).__deviceLab).toBeUndefined();

    // When the lab is installed
    const lab = installDeviceLab();

    // Then it is the global the driver reads, versioned by the sentinel, with nothing recorded yet
    expect((globalThis as LabScope).__deviceLab).toBe(lab);
    expect(lab.version).toBe(DEVICE_LAB_SENTINEL);
    expect(DEVICE_LAB_SENTINEL).toBe('__device_lab_v1__');
    expect(lab.calls).toEqual([]);
    expect(lab.alerts).toEqual([]);
  });

  it('keeps what was recorded when it is installed again', () => {
    // Given an installed lab that has recorded a call
    const first = installDeviceLab();
    first.calls.push({ api: 'vibrate', args: [200], at: 1 });

    // When the lab is installed a second time
    const second = installDeviceLab();

    // Then it is the same lab, with the call still there
    expect(second).toBe(first);
    expect(second.calls).toHaveLength(1);
  });
});
