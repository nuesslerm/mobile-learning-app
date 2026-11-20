import { useEffect, type ReactNode } from 'react';

import { PyodideRunnerProvider, useOptionalPyodideCodeRunner } from '../data/pyodide/PyodideRunnerProvider';
import { useRunnerSettingStore } from '../state/runnerSettingStore';

/** Starts the interpreter as soon as the device is the chosen runner (at launch with a saved choice, or when it is picked), so the first run is not the one that waits for the boot. */
function WarmOnChoice() {
  const runner = useOptionalPyodideCodeRunner();
  const choice = useRunnerSettingStore((s) => s.runner);
  useEffect(() => {
    if (choice === 'device') runner?.warmUp?.();
  }, [choice, runner]);
  return null;
}

/** The app root's mount of the on-device runner: the provider, and the warm-up that follows the setting. */
export function DeviceRunnerRoot({ children }: { children: ReactNode }) {
  return (
    <PyodideRunnerProvider>
      <WarmOnChoice />
      {children}
    </PyodideRunnerProvider>
  );
}
