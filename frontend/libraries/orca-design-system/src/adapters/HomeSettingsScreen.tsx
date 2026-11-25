// Ported from `references/orca/mobile/src/settings/settings-screen-frame.tsx` at
// `980c0024741812bd7ae934c0c22144d9926a784e` (Orca, MIT, Lovecast Inc.).
import type { HomeSettingsScreenProps } from '@lib/agent-ui-contracts';
import { Pressable, ScrollView, Text, View } from 'react-native';

import { homePalettes, useOrcaScheme } from './appearance';
import { BackIcon } from './homeIcons';
import { useHomeTabStyles } from './homeTabStyles';

/** A settings screen: the bar with its back button and title, then its rows. */
export function HomeSettingsScreen(props: HomeSettingsScreenProps) {
  const styles = useHomeTabStyles();
  const colors = homePalettes[useOrcaScheme()];
  return (
    <View style={[styles.settingsScreen, { backgroundColor: colors.bgBase }]}>
      <View style={styles.settingsNavBar}>
        <Pressable
          style={styles.settingsBack}
          accessibilityRole="button"
          accessibilityLabel="Back"
          onPress={props.onBack}
        >
          <BackIcon size={22} color={colors.textPrimary} />
        </Pressable>
        <Text style={styles.settingsNavTitle} accessibilityRole="header" numberOfLines={1}>
          {props.title}
        </Text>
      </View>
      <ScrollView
        contentContainerStyle={styles.settingsContent}
        showsVerticalScrollIndicator={false}
      >
        {props.children}
      </ScrollView>
    </View>
  );
}
