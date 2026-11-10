import type { ThreadFeedEntry } from '../../domain';
import { Card, Text, View } from '@lib/tamagui-design-system';

export type UserMessageItemProps = {
  entry: Extract<ThreadFeedEntry, { kind: 'user-message' }>;
};

export function UserMessageItem({
  entry,
}: UserMessageItemProps): React.ReactElement {
  return (
    <View items="flex-end">
      <Card tone="accent" testID={`UserMessage-${entry.id}`} maxW="85%">
        <Text>{entry.message.text}</Text>
      </Card>
    </View>
  );
}