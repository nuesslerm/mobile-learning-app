import { useState } from 'react';
import type { UserInputQuestion } from '@app/remote-chat-agent-contracts/orchestration';
import { type PendingUserInput } from '../domain';
import {
  Button,
  Card,
  Chip,
  Icon,
  Text,
  View,
} from '@lib/tamagui-design-system';
import { MessageSquare } from '@lib/tamagui-design-system/icons';

export type PendingUserInputCardProps = {
  pending: PendingUserInput;
  onRespond: (requestId: string, answers: Record<string, unknown>) => void;
};

type Selection = Record<string, string[]>;

function isSelected(
  selection: Selection,
  questionId: string,
  label: string,
): boolean {
  return (selection[questionId] ?? []).includes(label);
}

function toggleOption(
  selection: Selection,
  question: UserInputQuestion,
  label: string,
): Selection {
  const current = selection[question.id] ?? [];
  const has = current.includes(label);
  if (question.multiSelect) {
    return {
      ...selection,
      [question.id]: has
        ? current.filter((entry) => entry !== label)
        : [...current, label],
    };
  }
  return { ...selection, [question.id]: has ? [] : [label] };
}

function buildAnswers(
  questions: readonly UserInputQuestion[],
  selection: Selection,
): Record<string, string | string[]> {
  const answers: Record<string, string | string[]> = {};
  for (const question of questions) {
    const selected = selection[question.id] ?? [];
    answers[question.id] = question.multiSelect ? selected : selected[0];
  }
  return answers;
}

export function PendingUserInputCard({
  pending,
  onRespond,
}: PendingUserInputCardProps): React.ReactElement {
  const { requestId, questions } = pending;
  const [selection, setSelection] = useState<Selection>({});
  const allAnswered = questions.every(
    (question) => (selection[question.id] ?? []).length > 0,
  );

  return (
    <Card tone="default" testID={`UserInputCard-${requestId}`} gap="$2">
      <View flexDirection="row" items="center" gap="$1.5">
        <Icon as={MessageSquare} size={15} color="$color11" />
        <Text variant="strong">User input needed</Text>
      </View>
      {questions.map((question) => (
        <QuestionBlock
          key={question.id}
          question={question}
          requestId={requestId}
          selection={selection}
          onToggle={(label) =>
            setSelection((current) => toggleOption(current, question, label))
          }
        />
      ))}
      <Button
        disabled={!allAnswered}
        testID={`UserInputCardSubmit-${requestId}`}
        onPress={() => onRespond(requestId, buildAnswers(questions, selection))}
      >
        Submit
      </Button>
    </Card>
  );
}

function QuestionBlock({
  question,
  requestId,
  selection,
  onToggle,
}: {
  question: UserInputQuestion;
  requestId: string;
  selection: Selection;
  onToggle: (label: string) => void;
}): React.ReactElement {
  return (
    <View gap="$1.5">
      <Text variant="strong">
        {question.header} {question.multiSelect ? '· select all that apply' : ''}
      </Text>
      <Text size="$3">{question.question}</Text>
      <View flexDirection="row" gap="$1.5" flexWrap="wrap">
        {question.options.map((option, optionIndex) => (
          <Chip
            key={option.label}
            label={option.label}
            selected={isSelected(selection, question.id, option.label)}
            size="sm"
            pressLog="off"
            onPress={() => onToggle(option.label)}
            testID={`UserInputCardOption-${requestId}-${optionIndex}`}
          />
        ))}
      </View>
    </View>
  );
}