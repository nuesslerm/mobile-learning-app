// The app's update rows, as Orca's settings menu carries them.
//
// Ported from `references/orca/mobile/src/settings/app-update-settings-rows.tsx`
// at `980c0024741812bd7ae934c0c22144d9926a784e` (Orca, MIT, Lovecast Inc.).
// The version is this build's own, and an update is offered only when a check
// found one.

import {
  HOME_SETTINGS_SEPARATOR_INSET_PLAIN,
  useAgentComponents,
} from '@lib/agent-ui-contracts';
import { useEffect, useState } from 'react';

import { useAppInfo } from '../../state/useAppInfo';
import { useAppUpdate, type AppUpdateStatus } from '../../state/useAppUpdate';

const RELATIVE_TIME_TICK_MS = 60_000;

function checkRowValue(
  status: AppUpdateStatus,
  lastCheckedAt: number | null,
  now: number
): string {
  if (status === 'checking') {
    return 'Checking…';
  }
  if (status === 'up-to-date') {
    return 'Up to date';
  }
  if (status === 'failed') {
    return "Couldn't check";
  }
  if (lastCheckedAt === null) {
    return 'Never';
  }
  const delta = now - lastCheckedAt;
  if (delta < 60_000) {
    return 'Last checked just now';
  }
  const minutes = Math.max(0, Math.floor(delta / 60_000));
  if (minutes < 60) {
    return `Last checked ${minutes}m ago`;
  }
  const hours = Math.floor(minutes / 60);
  if (hours < 24) {
    return `Last checked ${hours}h ago`;
  }
  const days = Math.floor(hours / 24);
  return `Last checked ${days}d ago`;
}

/** The menu's middle group: what this build is, and whether a newer one exists. */
export function OrcaAppUpdateSection() {
  const { HomeSettingsGroup, HomeSettingsRow } = useAgentComponents();
  const { version } = useAppInfo();
  const { status, availableUpdateId, lastCheckedAt, check, install } = useAppUpdate();
  // Tick only when the relative label can age past just now.
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    const tick = setInterval(() => setNow(Date.now()), RELATIVE_TIME_TICK_MS);
    return () => clearInterval(tick);
  }, []);

  const checkValue = checkRowValue(status, lastCheckedAt, now);
  return (
    <HomeSettingsGroup separatorInset={HOME_SETTINGS_SEPARATOR_INSET_PLAIN}>
      {availableUpdateId ? (
        <HomeSettingsRow
          label="Update available"
          accessory="Update"
          onPress={install}
        />
      ) : version ? (
        <HomeSettingsRow label={`Version ${version}`} />
      ) : null}
      <HomeSettingsRow
        label="Check for updates"
        value={checkValue}
        accessibilityLabel={`Check for updates, ${checkValue}`}
        disabled={status === 'checking'}
        onPress={check}
      />
    </HomeSettingsGroup>
  );
}
