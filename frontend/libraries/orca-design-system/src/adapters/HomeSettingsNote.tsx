// Ported from `references/orca/mobile/src/settings/settings-screen-frame.tsx` at
// `980c0024741812bd7ae934c0c22144d9926a784e` (Orca, MIT, Lovecast Inc.).
import type { HomeSettingsNoteProps } from '@lib/agent-ui-contracts';
import { Text } from 'react-native';

import { homePalettes, useOrcaScheme } from './appearance';
import { useHomeTabStyles } from './homeTabStyles';

/** Grey text under a group: what the group means, or why it is empty. */
export function HomeSettingsNote(props: HomeSettingsNoteProps) {
  const styles = useHomeTabStyles();
  const colors = homePalettes[useOrcaScheme()];
  return <Text style={[styles.settingsNote, { color: colors.textSecondary }]}>{props.children}</Text>;
}
