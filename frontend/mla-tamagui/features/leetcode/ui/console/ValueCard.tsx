import { Text } from '@lib/tamagui-design-system';
import { YStack } from '@lib/tamagui-design-system/primitives';
import { useState } from 'react';
import { Pressable } from 'react-native';

import { clip } from '../../domain/run/caseDisplay';

export const INSET_VALUE_SURFACE = { px: 16, py: 8, rounded: '$3', bg: '$color4' } as const;
export const INSET_VALUE_CAPTION_OPACITY = 0.4;
export const INSET_VALUE_TYPOGRAPHY = { fontSize: 16, lineHeight: 26 } as const;
export const DARK_INSET_VALUE_TEXT = { color: '#fff', fontWeight: '500' } as const;

/** A caption over a monospace value on a card; a value longer than a line is clipped and a tap opens it. */
export function ValueCard({ id, caption, value, tone, inset = false, surfaceColor }: { id: string; caption: string; value: string; tone?: '$red10'; inset?: boolean; surfaceColor?: string }) {
  const [open, setOpen] = useState(false);
  const shown = open ? { text: value, clipped: true } : clip(value);
  const darkSurface = inset && surfaceColor !== undefined;
  return (
    <YStack gap="$1" testID={`${id}-Card`} {...(inset ? INSET_VALUE_SURFACE : {})} style={surfaceColor ? { backgroundColor: surfaceColor } : undefined}>
      <Text variant="muted" fontSize={inset ? 11 : undefined} opacity={darkSurface ? INSET_VALUE_CAPTION_OPACITY : undefined} testID={`${id}-Caption`}>
        {caption}
      </Text>
      <Pressable disabled={!shown.clipped} onPress={() => setOpen((was) => !was)} accessibilityRole={shown.clipped ? 'button' : undefined} testID={`${id}-Toggle`}>
        <YStack {...(!inset ? { px: '$3', py: '$2.5', rounded: '$3', bg: '$color3' } : {})}>
          <Text variant={inset ? undefined : 'code'} color={tone ?? (darkSurface ? DARK_INSET_VALUE_TEXT.color : undefined)} fontWeight={darkSurface ? DARK_INSET_VALUE_TEXT.fontWeight : undefined} {...(inset ? INSET_VALUE_TYPOGRAPHY : {})} testID={id}>
            {shown.text}
          </Text>
        </YStack>
      </Pressable>
    </YStack>
  );
}
