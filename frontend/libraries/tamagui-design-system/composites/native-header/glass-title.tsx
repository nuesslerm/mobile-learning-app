import {
  GlassView,
  isGlassEffectAPIAvailable,
  isLiquidGlassAvailable,
} from 'expo-glass-effect';
import { StyleSheet, View } from 'react-native';

import { Text } from '../../ui/text';

import type { HeaderTitleProps } from './screen-header-options';

export const GLASS_TITLE_ID = 'ScreenHeaderGlassTitle';

/** The title and the subtitle as one glass capsule, the way the Claude app's bar draws its title; below iOS 26 the same two lines show with no capsule. */
export function GlassTitle({ title, subtitle, maxWidth }: HeaderTitleProps) {
  const glass = isLiquidGlassAvailable() && isGlassEffectAPIAvailable();
  const Capsule = glass ? GlassView : View;
  return (
    <Capsule
      {...(glass ? { glassEffectStyle: 'regular' as const } : {})}
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
    </Capsule>
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
