import { Logger } from '@lib/logging';
import { Button, Text } from '@lib/tamagui-design-system';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';

import { useAutocompleteSettingStore } from '../state/autocompleteSettingStore';

const CHOICES: { value: boolean; label: string; testID: string }[] = [
  { value: false, label: 'Off', testID: 'LeetcodeListAutocompleteOff' },
  { value: true, label: 'On', testID: 'LeetcodeListAutocompleteOn' },
];

/** Whether the editor suggests names while typing: Python names in scope, keywords and the problem's own method and parameters. */
export function AutocompleteSettingRow() {
  const autocomplete = useAutocompleteSettingStore((s) => s.autocomplete);
  const setAutocomplete = useAutocompleteSettingStore((s) => s.setAutocomplete);
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
      testID="LeetcodeListAutocompleteRow"
    >
      <YStack>
        <Text variant="strong" testID="LeetcodeListAutocompleteLabel">
          Suggestions
        </Text>
        <Text variant="muted" testID="LeetcodeListAutocompleteHint">
          The editor offers names as you type, above the line. Tap one, or the Indent key, to take it. Return never takes it.
        </Text>
      </YStack>
      <XStack gap="$2" role="radiogroup" aria-label="Suggestions while typing" testID="LeetcodeListAutocomplete">
        {CHOICES.map(({ value, label, testID }) => (
          <Button
            key={testID}
            flex={1}
            size="$3"
            variant={autocomplete === value ? 'default' : 'outline'}
            role="radio"
            aria-checked={autocomplete === value}
            pressLog="off"
            testID={testID}
            onPress={() => {
              Logger.info('leetcode.list.autocomplete_tap', { autocomplete: value });
              if (autocomplete !== value) setAutocomplete(value);
            }}
          >
            {label}
          </Button>
        ))}
      </XStack>
    </YStack>
  );
}
