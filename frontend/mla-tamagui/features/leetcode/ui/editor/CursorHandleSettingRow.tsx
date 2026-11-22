import { Logger } from '@lib/logging';
import { Button, Text } from '@lib/tamagui-design-system';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';

import { useCursorHandleSettingStore } from '../../state/editor/cursorHandleSettingStore';

const CHOICES: { value: boolean; label: string; testID: string }[] = [
  { value: false, label: 'Off', testID: 'LeetcodeListCursorHandleOff' },
  { value: true, label: 'On', testID: 'LeetcodeListCursorHandleOn' },
];

/** Whether the keyboard's action row starts with a pad that drags the caret: sideways by characters and then words, up and down by lines. */
export function CursorHandleSettingRow() {
  const cursorHandle = useCursorHandleSettingStore((s) => s.cursorHandle);
  const setCursorHandle = useCursorHandleSettingStore((s) => s.setCursorHandle);
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
      testID="LeetcodeListCursorHandleRow"
    >
      <YStack>
        <Text variant="strong" testID="LeetcodeListCursorHandleLabel">
          Cursor handle
        </Text>
        <Text variant="muted" testID="LeetcodeListCursorHandleHint">
          A pad at the left end of the action row above the keyboard. Drag it sideways to move the caret by characters and then by words, and up or down by lines. With text selected, it moves the end of the selection.
        </Text>
      </YStack>
      <XStack gap="$2" role="radiogroup" aria-label="Cursor handle on the keyboard row" testID="LeetcodeListCursorHandle">
        {CHOICES.map(({ value, label, testID }) => (
          <Button
            key={testID}
            flex={1}
            size="$3"
            variant={cursorHandle === value ? 'default' : 'outline'}
            role="radio"
            aria-checked={cursorHandle === value}
            pressLog="off"
            testID={testID}
            onPress={() => {
              Logger.info('leetcode.list.cursor_handle_tap', { cursorHandle: value });
              if (cursorHandle !== value) setCursorHandle(value);
            }}
          >
            {label}
          </Button>
        ))}
      </XStack>
    </YStack>
  );
}
