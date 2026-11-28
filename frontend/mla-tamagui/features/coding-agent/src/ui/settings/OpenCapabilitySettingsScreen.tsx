// The screens for the settings the original offers and this app cannot back.
//
// The screen says so, and lists what the original offers, instead of showing
// controls that would do nothing.

import {
  HOME_SETTINGS_SEPARATOR_INSET_PLAIN,
  useSkin,
} from '@lib/skin';

import type { SettingsCapability } from '../../domain/settingsCapabilities';
import { OrcaSettingsScreen } from './OrcaSettingsScreen';

export function OpenCapabilitySettingsScreen(props: {
  title: string;
  note: string;
  capabilities: readonly SettingsCapability[];
}) {
  const { HomeSettingsGroup, HomeSettingsRow, HomeSettingsNote } = useSkin();
  return (
    <OrcaSettingsScreen title={props.title}>
      <HomeSettingsNote>{props.note}</HomeSettingsNote>
      <HomeSettingsGroup separatorInset={HOME_SETTINGS_SEPARATOR_INSET_PLAIN}>
        {props.capabilities.map((capability) => (
          <HomeSettingsRow
            key={capability.label}
            label={capability.label}
            value={capability.reason}
          />
        ))}
      </HomeSettingsGroup>
    </OrcaSettingsScreen>
  );
}
