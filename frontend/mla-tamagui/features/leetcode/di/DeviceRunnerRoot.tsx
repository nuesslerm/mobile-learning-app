import type { ReactNode } from 'react';

import { DeviceRunnerProvider } from '../data/runner/DeviceRunnerProvider';

/** The app root's mount of the on-device runner. It loads nothing at launch: the editor warms it when it opens a problem, and a run loads it if that has not happened yet. */
export function DeviceRunnerRoot({ children }: { children: ReactNode }) {
  return <DeviceRunnerProvider>{children}</DeviceRunnerProvider>;
}
