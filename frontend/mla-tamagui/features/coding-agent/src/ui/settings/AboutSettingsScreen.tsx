// This build's identity, as the original's About screen offers it.
//
// The version and build come from the build itself, so they are always true.
// The original also links its own project pages; those are not shown here,
// because they would point at another product's pages.

import {
  HOME_SETTINGS_SEPARATOR_INSET_PLAIN,
  useSkin,
} from '@lib/skin';

import { useAppInfo } from '../../state/useAppInfo';
import { OrcaSettingsScreen } from './OrcaSettingsScreen';

export function AboutSettingsScreen() {
  const { HomeSettingsGroup, HomeSettingsRow } = useSkin();
  const { version, build } = useAppInfo();
  const label = version === null ? null : build === null ? `v${version}` : `v${version} (${build})`;
  return (
    <OrcaSettingsScreen title="About">
      <HomeSettingsGroup separatorInset={HOME_SETTINGS_SEPARATOR_INSET_PLAIN}>
        {label === null ? null : <HomeSettingsRow label="Version" value={label} />}
      </HomeSettingsGroup>
    </OrcaSettingsScreen>
  );
}
