import { Logger } from '@lib/logging';
import { Button, Text } from '@lib/tamagui-design-system';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';

import { useBlockViewSettingStore } from '../state/blockViewSettingStore';

const CHOICES: { value: boolean; label: string; testID: string }[] = [
  { value: false, label: 'Off', testID: 'LeetcodeListBlockViewOff' },
  { value: true, label: 'On', testID: 'LeetcodeListBlockViewOn' },
];

/** Whether the editor screen shows the code as blocks under the editor (Python): rows to drag, and a palette to drag new statements from. */
export function BlockViewSettingRow() {
  const blockView = useBlockViewSettingStore((s) => s.blockView);
  const setBlockView = useBlockViewSettingStore((s) => s.setBlockView);
  return (
    <YStack
      gap="$2.5"
      mt="$3"
      mx="$4"
      px="$3.5"
      py="$3"
      rounded="$4"
      borderWidth={1}
      borderColor="$borderColor"
      bg="$background"
      testID="LeetcodeListBlockViewRow"
    >
      <YStack>
        <Text variant="strong" testID="LeetcodeListBlockViewLabel">
          Blocks (experimental)
        </Text>
        <Text variant="muted" testID="LeetcodeListBlockViewHint">
          Shows the Python code as rows under the editor. Drag a row by its grip to move a statement, or a chip to add one. A line that does not parse stays plain text.
        </Text>
      </YStack>
      <XStack gap="$2" role="radiogroup" aria-label="Block view of the code" testID="LeetcodeListBlockView">
        {CHOICES.map(({ value, label, testID }) => (
          <Button
            key={testID}
            flex={1}
            size="$3"
            variant={blockView === value ? 'default' : 'outline'}
            role="radio"
            aria-checked={blockView === value}
            pressLog="off"
            testID={testID}
            onPress={() => {
              Logger.info('leetcode.list.block_view_tap', { blockView: value });
              if (blockView !== value) setBlockView(value);
            }}
          >
            {label}
          </Button>
        ))}
      </XStack>
    </YStack>
  );
}
