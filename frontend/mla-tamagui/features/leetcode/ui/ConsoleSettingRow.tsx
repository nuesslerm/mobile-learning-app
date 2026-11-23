import { Logger } from '@lib/logging';
import { Button, Text } from '@lib/tamagui-design-system';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';

import { useConsoleSettingStore } from '../state/consoleSettingStore';

const CHOICES: { value: boolean; label: string; testID: string }[] = [
  { value: false, label: 'Off', testID: 'LeetcodeListConsoleOff' },
  { value: true, label: 'On', testID: 'LeetcodeListConsoleOn' },
];

/** Whether the problem screen shows the console (Testcase and Run Result tabs, Run and Submit) in place of the Run Local card. */
export function ConsoleSettingRow() {
  const enabled = useConsoleSettingStore((s) => s.enabled);
  const setEnabled = useConsoleSettingStore((s) => s.setEnabled);
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
      testID="LeetcodeListConsoleRow"
    >
      <YStack>
        <Text variant="strong" testID="LeetcodeListConsoleLabel">
          Console
        </Text>
        <Text variant="muted" testID="LeetcodeListConsoleHint">
          On gives a bottom console with your testcases and the run result. Off keeps the Run Local button and its card.
        </Text>
      </YStack>
      <XStack gap="$2" role="radiogroup" aria-label="Console" testID="LeetcodeListConsole">
        {CHOICES.map(({ value, label, testID }) => (
          <Button
            key={testID}
            flex={1}
            size="$3"
            variant={enabled === value ? 'default' : 'outline'}
            role="radio"
            aria-checked={enabled === value}
            pressLog="off"
            testID={testID}
            onPress={() => {
              Logger.info('leetcode.list.console_tap', { enabled: value });
              if (enabled !== value) setEnabled(value);
            }}
          >
            {label}
          </Button>
        ))}
      </XStack>
    </YStack>
  );
}
