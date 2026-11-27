import type { HomeSettingsNoteProps } from '@lib/skin';
import { Text } from '@lib/tamagui-design-system';

/** Grey text under or above a group: what the group means, or why it is empty. */
export function HomeSettingsNote(props: HomeSettingsNoteProps) {
  return (
    <Text variant="muted" mx="$4">
      {props.children}
    </Text>
  );
}
