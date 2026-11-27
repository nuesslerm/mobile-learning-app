// The coding agent's own notification opt-in.
//
// The choice is kept on the device. Nothing is sent: this build has no push
// service, so the opt-in records what the user asked for and the screen says
// what is still missing.

import {
  HOME_SETTINGS_SEPARATOR_INSET_PLAIN,
  useSkin,
} from '@lib/skin';
import { useCallback, useEffect, useState } from 'react';

import { useCodingAgentPorts } from '../../state/ports';
import { OrcaSettingsScreen } from './OrcaSettingsScreen';

export function NotificationsSettingsScreen() {
  const { HomeSettingsGroup, HomeSettingsRow, HomeSettingsNote } = useSkin();
  const { preferences } = useCodingAgentPorts();
  const [enabled, setEnabled] = useState(false);

  useEffect(() => {
    let active = true;
    void preferences.readNotificationsEnabled().then((stored) => {
      if (active) {
        setEnabled(stored);
      }
    });
    return () => {
      active = false;
    };
  }, [preferences]);

  const toggle = useCallback(
    (next: boolean) => {
      setEnabled(next);
      void preferences.writeNotificationsEnabled(next);
    },
    [preferences]
  );

  return (
    <OrcaSettingsScreen title="Notifications">
      <HomeSettingsNote>
        Agent alerts reach this device while it is open. Delivering them with
        the app closed needs a push service, which this build does not carry.
      </HomeSettingsNote>
      <HomeSettingsGroup separatorInset={HOME_SETTINGS_SEPARATOR_INSET_PLAIN}>
        <HomeSettingsRow
          label="Enable notifications"
          toggle={{
            value: enabled,
            onValueChange: toggle,
            accessibilityLabel: 'Enable notifications',
          }}
        />
      </HomeSettingsGroup>
    </OrcaSettingsScreen>
  );
}
