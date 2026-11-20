import { Logger } from '@lib/logging';
import { Button, Text } from '@lib/tamagui-design-system';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';

import type { RunnerChoice } from '../domain/ports';
import { useRunnerSettingStore } from '../state/runnerSettingStore';

const CHOICES: { value: RunnerChoice; label: string }[] = [
  { value: 'backend', label: 'Server' },
  { value: 'device', label: 'This device' },
];

/**
 * Where Run Local executes. Only Python runs on the device; every other language goes to the backend whatever this says.
 * Two buttons, not a toggle group: the group drew an empty box on the Android emulator (the hosted `local-run` could not find its items).
 */
export function RunnerSettingRow() {
  const runner = useRunnerSettingStore((s) => s.runner);
  const setRunner = useRunnerSettingStore((s) => s.setRunner);
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
      testID="LeetcodeListRunnerRow"
    >
      <YStack>
        <Text variant="strong" testID="LeetcodeListRunnerLabel">
          Run code
        </Text>
        <Text variant="muted" testID="LeetcodeListRunnerHint">
          On this device runs Python with no network. Other languages run on the server.
        </Text>
      </YStack>
      <XStack gap="$2" role="radiogroup" aria-label="Where Run Local runs" testID="LeetcodeListRunner">
        {CHOICES.map(({ value, label }) => (
          <Button
            key={value}
            flex={1}
            size="$3"
            variant={runner === value ? 'default' : 'outline'}
            // a radio with its checked state: React Native maps these to the screen reader's role and state (it has no aria-pressed)
            role="radio"
            aria-checked={runner === value}
            pressLog="off"
            testID={value === 'backend' ? 'LeetcodeListRunnerBackend' : 'LeetcodeListRunnerDevice'}
            onPress={() => {
              // the chosen one tapped again keeps the choice; the tap is logged either way, so a test sees that it reached the screen
              Logger.info('leetcode.list.runner_tap', { runner: value });
              if (runner !== value) setRunner(value);
            }}
          >
            {label}
          </Button>
        ))}
      </XStack>
    </YStack>
  );
}
