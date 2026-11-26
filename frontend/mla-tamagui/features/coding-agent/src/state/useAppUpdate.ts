// features/coding-agent/src/state/useAppUpdate.ts
//
// The app's update channel, as the settings menu shows it: what the last
// check found, when it ran, and the two actions the user can take.
//
// Nothing here runs on its own: a check starts when the user asks for one,
// and an update is fetched only when the user asks for that.

import { useCallback, useEffect, useState } from 'react';

import type { AppUpdateCheck } from '../domain/ports';
import { useCodingAgentPorts } from './ports';

export type AppUpdateStatus =
  | 'idle'
  | 'checking'
  | 'available'
  | 'up-to-date'
  | 'failed';

export interface AppUpdateState {
  status: AppUpdateStatus;
  /** The update a check found, or null when there is none. */
  availableUpdateId: string | null;
  /** When the user last asked for a check, or null when never. */
  lastCheckedAt: number | null;
  /** The user asked for a check. */
  check: () => void;
  /** The user asked for the update to be applied. */
  install: () => void;
}

export function useAppUpdate(): AppUpdateState {
  const { appUpdate, preferences } = useCodingAgentPorts();
  const [status, setStatus] = useState<AppUpdateStatus>('idle');
  const [availableUpdateId, setAvailableUpdateId] = useState<string | null>(null);
  const [lastCheckedAt, setLastCheckedAt] = useState<number | null>(null);

  useEffect(() => {
    let active = true;
    void preferences.readLastUpdateCheckAt().then((at) => {
      if (active) {
        setLastCheckedAt(at);
      }
    });
    return () => {
      active = false;
    };
  }, [preferences]);

  const check = useCallback(() => {
    setStatus('checking');
    void appUpdate.checkNow().then((outcome: AppUpdateCheck) => {
      const checkedAt = Date.now();
      void preferences.writeLastUpdateCheckAt(checkedAt);
      setLastCheckedAt(checkedAt);
      if (outcome.status === 'available') {
        setAvailableUpdateId(outcome.updateId);
        setStatus('available');
        return;
      }
      setAvailableUpdateId(null);
      setStatus(outcome.status === 'up-to-date' ? 'up-to-date' : 'failed');
    });
  }, [appUpdate, preferences]);

  const install = useCallback(() => {
    if (!availableUpdateId) {
      return;
    }
    setStatus('checking');
    void appUpdate.install(availableUpdateId).catch(() => setStatus('failed'));
  }, [appUpdate, availableUpdateId]);

  return { status, availableUpdateId, lastCheckedAt, check, install };
}
