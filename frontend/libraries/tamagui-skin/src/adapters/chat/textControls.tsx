import { useChatText } from '@lib/skin';
import { Button, IconButton, Text } from '@lib/tamagui-design-system';
import { Minus, Plus } from '@lib/tamagui-design-system/icons';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';

const SMALLEST = 11;
const LARGEST = 24;

/** The reader's text size, a step at a time. */
export function ChatSizeControl({ label }: { label: string }) {
  const { value, defaultSize, update } = useChatText();
  const select = (size: number) => update({ size: Math.max(SMALLEST, Math.min(LARGEST, size)) });
  return (
    <XStack items="center" gap="$3" px="$4" py="$2">
      <Text flex={1}>{label}</Text>
      <IconButton
        icon={Minus}
        variant="secondary"
        glass={false}
        size="$3"
        accessibilityLabel="Smaller text"
        disabled={value.size <= SMALLEST}
        pressLog="off"
        onPress={() => select(value.size - 1)}
        testID="skin-chat-size-smaller"
      />
      <Text color="$color11" minW={64} text="center" aria-label="Chat text size">
        {value.size === defaultSize ? 'Default' : `${value.size} pt`}
      </Text>
      <IconButton
        icon={Plus}
        variant="secondary"
        glass={false}
        size="$3"
        accessibilityLabel="Larger text"
        disabled={value.size >= LARGEST}
        pressLog="off"
        onPress={() => select(value.size + 1)}
        testID="skin-chat-size-larger"
      />
    </XStack>
  );
}

const SPACINGS = ['compact', 'default', 'relaxed'] as const;

export function ChatSpacingControl({ label = false }: { label?: boolean }) {
  const { value, update } = useChatText();
  return (
    <YStack gap="$2" px="$4" py="$2">
      {label ? <Text>Line spacing</Text> : null}
      <XStack gap="$2">
        {SPACINGS.map((lineSpacing) => {
          const selected = value.lineSpacing === lineSpacing;
          return (
            <Button
              key={lineSpacing}
              flex={1}
              size="$3"
              variant={selected ? 'secondary' : 'ghost'}
              glass={false}
              aria-selected={selected}
              pressLog="off"
              onPress={() => update({ lineSpacing })}
              testID={`skin-chat-spacing-${lineSpacing}`}
            >
              {lineSpacing[0].toUpperCase() + lineSpacing.slice(1)}
            </Button>
          );
        })}
      </XStack>
    </YStack>
  );
}
