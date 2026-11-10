import { useState } from 'react';
import type { ThreadFeedEntry } from '../../domain';
import { Chip, Markdown, Text, View } from '@lib/tamagui-design-system';
import { Brain } from '@lib/tamagui-design-system/icons';

export type AssistantMessageItemProps = {
  entry: Extract<ThreadFeedEntry, { kind: 'assistant-message' }>;
  reasoning?: string;
  streaming?: boolean;
};

export function AssistantMessageItem({
  entry,
  reasoning,
  streaming = false,
}: AssistantMessageItemProps): React.ReactElement {
  const [reasoningExpanded, setReasoningExpanded] = useState(false);
  const hasReasoning = reasoning !== undefined && reasoning.length > 0;

  return (
    <View gap="$1.5">
      {hasReasoning ? (
        <>
          <Chip
            icon={Brain}
            label="Reasoning"
            size="sm"
            selected={reasoningExpanded}
            onPress={() => setReasoningExpanded((current) => !current)}
            testID={`AssistantMessageReasoningToggle-${entry.id}`}
          />
          {reasoningExpanded ? (
            <Text
              variant="muted"
              testID={`AssistantMessageReasoning-${entry.id}`}
            >
              {reasoning}
            </Text>
          ) : null}
        </>
      ) : null}
      <Markdown content={entry.message.text} />
      {streaming ? (
        <Text variant="muted" testID={`AssistantMessageCaret-${entry.id}`}>
          {'\u258D'}
        </Text>
      ) : null}
    </View>
  );
}