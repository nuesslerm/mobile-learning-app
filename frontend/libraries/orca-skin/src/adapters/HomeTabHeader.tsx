// Ported from `references/orca/mobile/src/home/tabs/MobileHomeTabHeader.tsx` at
// `980c0024741812bd7ae934c0c22144d9926a784e` (Orca, MIT, Lovecast Inc.).
import type {
  HomeTabHeaderAction,
  HomeTabHeaderProps,
} from '@lib/skin';
import { Pressable, Text, View } from 'react-native';

import { homePalettes, useOrcaScheme } from './appearance';
import { homeIcons } from './homeIcons';
import { useHomeTabStyles } from './homeTabStyles';

/** A Home tab's title row: the tab's name and at most two icon buttons. */
export function HomeTabHeader(props: HomeTabHeaderProps) {
  const styles = useHomeTabStyles();
  return (
    <View style={styles.header}>
      <Text style={styles.headerTitle} numberOfLines={1} accessibilityRole="header">
        {props.title}
      </Text>
      {props.actions && props.actions.length > 0 ? (
        <View style={styles.headerActions}>
          {props.actions.map((action) => (
            <HeaderButton key={action.id} action={action} />
          ))}
        </View>
      ) : null}
    </View>
  );
}

function HeaderButton({ action }: { action: HomeTabHeaderAction }) {
  const styles = useHomeTabStyles();
  const colors = homePalettes[useOrcaScheme()];
  const Icon = homeIcons[action.icon];
  return (
    <Pressable
      style={[
        styles.headerAction,
        action.active && styles.headerActionActive,
        action.disabled && styles.disabled,
      ]}
      accessibilityRole="button"
      accessibilityLabel={action.accessibilityLabel}
      accessibilityState={{
        disabled: Boolean(action.disabled),
        expanded: Boolean(action.active),
      }}
      disabled={action.disabled}
      onPress={action.onPress}
    >
      <Icon size={21} color={colors.textSecondary} />
    </Pressable>
  );
}
