import { GlassView } from 'expo-glass-effect';
import { StyleSheet } from 'react-native';

import { Text } from '../../ui/text';

import type { HeaderTitleProps } from './screen-header-options';

export const GLASS_TITLE_ID = 'ScreenHeaderGlassTitle';

/** The title and the subtitle as one glass capsule, the way the Claude app's bar draws its title; the native bar is only built where the glass API is there. */
export function GlassTitle({ title, subtitle, maxWidth }: HeaderTitleProps) {
  return (
    <GlassView
      glassEffectStyle="regular"
      style={[styles.capsule, { maxWidth }]}
      testID={GLASS_TITLE_ID}
    >
      <Text variant="large" numberOfLines={1} testID={`${GLASS_TITLE_ID}Text`}>
        {title}
      </Text>
      {subtitle ? (
        <Text
          variant="muted"
          numberOfLines={1}
          testID={`${GLASS_TITLE_ID}Subtitle`}
        >
          {subtitle}
        </Text>
      ) : null}
    </GlassView>
  );
}

const styles = StyleSheet.create({
  capsule: {
    alignItems: 'center',
    borderRadius: 22,
    paddingHorizontal: 16,
    paddingVertical: 4,
  },
});
