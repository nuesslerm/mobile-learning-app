import { Text } from '@lib/tamagui-design-system';
import { YStack } from '@lib/tamagui-design-system/primitives';
import { useState } from 'react';
import { Pressable } from 'react-native';

import { clip } from '../../domain/run/caseDisplay';

/** A caption over a monospace value on a card; a value longer than a line is clipped and a tap opens it. */
export function ValueCard({ id, caption, value, tone }: { id: string; caption: string; value: string; tone?: '$red10' }) {
  const [open, setOpen] = useState(false);
  const shown = open ? { text: value, clipped: true } : clip(value);
  return (
    <YStack gap="$1" testID={`${id}-Card`}>
      <Text variant="muted" testID={`${id}-Caption`}>
        {caption}
      </Text>
      <Pressable disabled={!shown.clipped} onPress={() => setOpen((was) => !was)} accessibilityRole={shown.clipped ? 'button' : undefined} testID={`${id}-Toggle`}>
        <YStack px="$3" py="$2.5" rounded="$3" bg="$color3">
          <Text variant="code" color={tone} testID={id}>
            {shown.text}
          </Text>
        </YStack>
      </Pressable>
    </YStack>
  );
}
