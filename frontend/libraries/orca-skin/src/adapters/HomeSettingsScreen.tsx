// Ported from `references/orca/mobile/src/settings/settings-screen-frame.tsx` at
// `2c0cbbc279b61afe80a954fbf43de4f34ef2c32c` (Orca, MIT, Lovecast Inc.).
import type { HomeSettingsScreenProps } from '@lib/skin';
import { Pressable, ScrollView, Text, View } from 'react-native';

import { homePalettes, useOrcaScheme } from './appearance';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { BackIcon } from './homeIcons';
import { useHomeTabStyles } from './homeTabStyles';

/** A settings screen: the bar with its back button and title, then its rows. */
export function HomeSettingsScreen(props: HomeSettingsScreenProps) {
  const styles = useHomeTabStyles();
  const { bottom } = useSafeAreaInsets();
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
          <BackIcon size={18} strokeWidth={2.1} color={colors.textPrimary} />
        </Pressable>
        <Text style={styles.settingsNavTitle} accessibilityRole="header" numberOfLines={1}>
          {props.title}
        </Text>
      </View>
      <ScrollView
        contentContainerStyle={[styles.settingsContent, { paddingBottom: bottom + 24 }]}
        showsVerticalScrollIndicator={false}
      >
        {props.children}
      </ScrollView>
    </View>
  );
}
