// features/coding-agent/src/state/useAppInfo.ts
//
// The app's own identity, as the settings screens show it.

import { useEffect, useState } from 'react';

import { useCodingAgentPorts } from './ports';

export interface AppInfo {
  /** The app's version, e.g. "1.0.0"; null when the build carries none. */
  version: string | null;
  /** The native build number; null when the build carries none. */
  build: string | null;
}

export function useAppInfo(): AppInfo {
  const { appInfo } = useCodingAgentPorts();
  const [info, setInfo] = useState<AppInfo>({ version: null, build: null });

  useEffect(() => {
    let active = true;
    void Promise.all([appInfo.version(), appInfo.build()]).then(([version, build]) => {
      if (active) {
        setInfo({ version, build });
      }
    });
    return () => {
      active = false;
    };
  }, [appInfo]);

  return info;
}
