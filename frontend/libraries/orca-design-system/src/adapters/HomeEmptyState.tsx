// Ported from the empty states of `references/orca/mobile/src/home/tabs/` at
// `980c0024741812bd7ae934c0c22144d9926a784e` (Orca, MIT, Lovecast Inc.).
import type { HomeEmptyStateProps } from '@lib/agent-ui-contracts';
import { Text, View } from 'react-native';

import { homePalettes, useOrcaScheme } from './appearance';
import { homeIcons } from './homeIcons';
import { useHomeTabStyles } from './homeTabStyles';

/** What a tab shows when it has nothing to show. */
export function HomeEmptyState(props: HomeEmptyStateProps) {
  const styles = useHomeTabStyles();
  const colors = homePalettes[useOrcaScheme()];
  const Icon = homeIcons[props.icon];
  return (
    <View style={styles.empty}>
      <Icon
        size={props.iconSize ?? 28}
        strokeWidth={props.iconStrokeWidth}
        color={colors.textMuted}
      />
      <Text
        style={[styles.emptyTitle, { fontSize: props.titleSize ?? 17, marginTop: props.iconGap ?? 8 }]}
        accessibilityRole="header"
      >
        {props.title}
      </Text>
      {props.message ? (
        <Text style={[styles.emptyBody, { fontSize: props.bodySize ?? 15, marginTop: props.titleGap ?? 4 }]}>
          {props.message}
        </Text>
      ) : null}
    </View>
  );
}
