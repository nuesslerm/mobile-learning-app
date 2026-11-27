import type { HomeSettingsScreenProps } from '@lib/skin';
import { ScreenHeader, View, useScrollUnder } from '@lib/tamagui-design-system';
import { ScrollView } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

/** A settings screen as the app's own Settings draws one: the app's header, then groups with even gaps. */
export function HomeSettingsScreen(props: HomeSettingsScreenProps) {
  const { bottom } = useSafeAreaInsets();
  const scrollUnder = useScrollUnder(true);
  return (
    <View flex={1} bg="$background" testID="skin-settings-screen">
      <ScreenHeader glass transparent testID="skin-settings-header" title={props.title} onBack={props.onBack} backAccessibilityLabel="Back" />
      <ScrollView
        {...scrollUnder}
        showsVerticalScrollIndicator={false}
        contentContainerStyle={{ paddingTop: 16, paddingBottom: bottom + 24, gap: 12 }}
      >
        {props.children}
      </ScrollView>
    </View>
  );
}
