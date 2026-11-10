import { Logger } from '@lib/logging';

import { installLabAlert } from './alert';

/**
 * The in-app half of device-lab: mocks for native APIs that neither the
 * browser nor Chromium emulation can supply (docs/plans/device-lab/contract.md,
 * "Lab module"). It ships only in a web export built with
 * EXPO_PUBLIC_DEVICE_LAB=1; the root layout reaches it through a gated
 * `require`, so a flagless build folds the branch away and never bundles it.
 * The sentinel string below is how a build proves which of the two it is.
 */
export const DEVICE_LAB_SENTINEL = '__device_lab_v1__';

type LabCall = { api: string; args: unknown[]; at: number };

type LabAlert = {
  title: string;
  message?: string;
  buttons: string[];
  answered?: number;
};

export interface DeviceLab {
  version: typeof DEVICE_LAB_SENTINEL;
  /** Recorded calls to APIs that are a no-op on web: vibrate, speech, share. */
  calls: LabCall[];
  /** Every alert shown, with the index of the button that answered it. */
  alerts: LabAlert[];
}

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
