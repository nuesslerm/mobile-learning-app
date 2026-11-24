import { Logger } from '@lib/logging';

import { installLabAlert } from './alert';
import { DEVICE_LAB_SENTINEL, type DeviceLab } from './types';

/**
 * The in-app half of device-lab: mocks for native APIs that neither the
 * browser nor Chromium emulation can supply (docs/plans/device-lab/contract.md,
 * "Lab module"). It ships only in a web export built with
 * EXPO_PUBLIC_DEVICE_LAB=1; the root layout reaches it through a gated
 * `require`, so a flagless build folds the branch away and never bundles it.
 * The sentinel string below is how a build proves which of the two it is.
 */
export { DEVICE_LAB_SENTINEL } from './types';
export type { DeviceLab } from './types';

type LabScope = typeof globalThis & { __deviceLab?: DeviceLab };

export function installDeviceLab(): DeviceLab {
  const scope = globalThis as LabScope;
  if (scope.__deviceLab?.version === DEVICE_LAB_SENTINEL) return scope.__deviceLab;
  const lab: DeviceLab = { version: DEVICE_LAB_SENTINEL, calls: [], alerts: [] };
  scope.__deviceLab = lab;
  installLabAlert(lab);
  Logger.info('device_lab.installed', { version: DEVICE_LAB_SENTINEL });
  return lab;
}
