import { BottomSheet } from './BottomSheet';
// Ported from the options menu of `references/orca/mobile/src/home/tabs/` at
// `980c0024741812bd7ae934c0c22144d9926a784e` (Orca, MIT, Lovecast Inc.).
import type {
  HomeActionSheetAction,
  HomeActionSheetProps,
} from '@lib/agent-ui-contracts';
import { Pressable, Text, View } from 'react-native';

import { homePalettes, useOrcaScheme } from './appearance';
import { useHomeTabStyles } from './homeTabStyles';

/** The native sheet the agents tab opens its options in. */
export function HomeActionSheet(props: HomeActionSheetProps) {
  const styles = useHomeTabStyles();
  return (
    props.visible && (
      <BottomSheet label="Close" onClose={props.onClose} style={styles.sheet}>
        {() => (
          <>
            {props.groups.map((group, groupIndex) => (
              <View key={groupIndex} style={styles.sheetGroup}>
                {group.map((action) => (
                  <SheetRow key={action.id} action={action} />
                ))}
              </View>
            ))}
          </>
        )}
      </BottomSheet>
    )
  );
}

function SheetRow({ action }: { action: HomeActionSheetAction }) {
  const styles = useHomeTabStyles();
  const colors = homePalettes[useOrcaScheme()];
  return (
    <Pressable
      style={[styles.sheetRow, action.disabled && styles.disabled]}
      accessibilityRole="button"
      accessibilityLabel={action.label}
      accessibilityState={{
        disabled: Boolean(action.disabled),
        checked: Boolean(action.checked),
      }}
      disabled={action.disabled}
      onPress={action.onPress}
    >
      <Text style={styles.sheetRowLabel}>{action.label}</Text>
      {action.checked ? (
        <Text style={[styles.sheetRowLabel, { color: colors.accentBlue }]}>
          ✓
        </Text>
      ) : null}
    </Pressable>
  );
}
