import type { HomeSettingsNoteProps } from '@lib/skin';
import { Text } from '@lib/tamagui-design-system';

/** Grey text under a group: what the group means, or why it is empty. */
export function HomeSettingsNote(props: HomeSettingsNoteProps) {
  return (
    <Text variant="muted" px="$2" py="$2">
      {props.children}
    </Text>
  );
}
