import { Logger } from '@lib/logging';
import { Text, ToggleGroup, ToggleGroupItem } from '@lib/tamagui-design-system';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';

import type { RunnerChoice } from '../domain/ports';
import { useRunnerSettingStore } from '../state/runnerSettingStore';

/** Where Run Local executes. Only Python runs on the device; every other language goes to the backend whatever this says. */
export function RunnerSettingRow() {
  const runner = useRunnerSettingStore((s) => s.runner);
  const setRunner = useRunnerSettingStore((s) => s.setRunner);
  return (
    <XStack
      items="center"
      justify="space-between"
      gap="$3"
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
      <YStack flex={1}>
        <Text variant="strong" testID="LeetcodeListRunnerLabel">
          Run code
        </Text>
        <Text variant="muted" testID="LeetcodeListRunnerHint">
          On this device runs Python with no network. Other languages run on the server.
        </Text>
      </YStack>
      <ToggleGroup
        type="single"
        size="sm"
        value={runner}
        onValueChange={(value) => {
          // a single toggle group reports an empty value when the chosen item is tapped again: the choice stays
          if (value !== 'backend' && value !== 'device') return;
          Logger.info('leetcode.list.runner_tap', { runner: value });
          setRunner(value satisfies RunnerChoice);
        }}
        testID="LeetcodeListRunner"
      >
        <ToggleGroupItem value="backend" testID="LeetcodeListRunnerBackend">
          <Text variant="small" testID="LeetcodeListRunnerBackendLabel">
            Server
          </Text>
        </ToggleGroupItem>
        <ToggleGroupItem value="device" testID="LeetcodeListRunnerDevice">
          <Text variant="small" testID="LeetcodeListRunnerDeviceLabel">
            This device
          </Text>
        </ToggleGroupItem>
      </ToggleGroup>
    </XStack>
  );
}
