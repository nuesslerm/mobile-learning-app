import { Button, Text } from '@lib/tamagui-design-system';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';
import type { ReactNode } from 'react';
import type { LayoutChangeEvent } from 'react-native';

import { customOutcomeOf } from '../../domain/run/customOutcome';
import type { ErrorLocation, RunResponse } from '../../domain/run/execution';
import { CustomCaseResult } from './CustomCaseResult';
import { RunCaseList } from './RunCaseList';

/** What the card on screen shows: a run of the bundled cases, or of one case the learner typed (with the inputs it ran on). */
export type RunCard = { mode: 'tests'; response: RunResponse } | { mode: 'custom'; response: RunResponse; inputs: Record<string, unknown> };

/** The line the run reported, when it did: the response's for the bundled cases, the typed case's own row for a custom card. */
export function errorLocationOf(card: RunCard): ErrorLocation | undefined {
  return card.mode === 'custom' ? customOutcomeOf(card.response, card.inputs).errorAt : card.response.error_at;
}

type OnLayout = (event: LayoutChangeEvent) => void;
type ShowLine = { line: number; onPress: () => void };

function Frame({ borderColor, onLayout, children }: { borderColor: '$green9' | '$red9' | '$borderColor'; onLayout?: OnLayout; children: ReactNode }) {
  return (
    <YStack mx="$3" mb="$1" px="$3" py="$2" rounded="$3" borderWidth={1} borderColor={borderColor} onLayout={onLayout} testID="ProblemEditorRunOutcome">
      {children}
    </YStack>
  );
}

/** The card's headline; with a marked error line it has a press target beside it that takes the learner back to that line. */
function Status({ text, color, showLine }: { text: string; color?: '$green10' | '$red10'; showLine?: ShowLine }) {
  const status = (
    <Text variant="strong" color={color} testID="ProblemEditorRunOutcomeStatus">
      {text}
    </Text>
  );
  if (!showLine) return status;
  return (
    <XStack items="center" justify="space-between" gap="$2">
      {status}
      <Button
        testID="ProblemEditorRunOutcomeLine"
        variant="link"
        size="$2"
        pressLog="off"
        accessibilityLabel={`Show line ${showLine.line} in the editor`}
        onPress={showLine.onPress}
      >
        {`Line ${showLine.line}`}
      </Button>
    </XStack>
  );
}

function CustomCard({ response, inputs, onLayout, showLine }: { response: RunResponse; inputs: Record<string, unknown>; onLayout?: OnLayout; showLine?: ShowLine }) {
  const outcome = customOutcomeOf(response, inputs);
  const failed = outcome.status === 'Error';
  return (
    <Frame borderColor={failed ? '$red9' : '$borderColor'} onLayout={onLayout}>
      <Status text={outcome.status} color={failed ? '$red10' : undefined} showLine={showLine} />
      <CustomCaseResult outcome={outcome} />
    </Frame>
  );
}

/**
 * The result of a run: for the bundled cases the status, totals, the failure text and every case; for a typed case what it returned.
 * `onShowLine` is given while the error line is marked in the editor; the card then offers "Line N" to go back to it.
 */
export function RunOutcomeCard({ card, onLayout, onShowLine }: { card: RunCard; onLayout?: OnLayout; onShowLine?: () => void }) {
  const line = onShowLine ? errorLocationOf(card)?.line : undefined;
  const showLine = onShowLine && line !== undefined ? { line, onPress: onShowLine } : undefined;
  if (card.mode === 'custom') return <CustomCard response={card.response} inputs={card.inputs} onLayout={onLayout} showLine={showLine} />;
  const outcome = card.response;
  const accepted = outcome.status_msg === 'Accepted';
  return (
    <Frame borderColor={accepted ? '$green9' : '$red9'} onLayout={onLayout}>
      <Status text={outcome.status_msg} color={accepted ? '$green10' : '$red10'} showLine={showLine} />
      {outcome.total_testcases > 0 ? (
        <Text variant="muted" testID="ProblemEditorRunOutcomeTotals">
          {`${outcome.total_correct} / ${outcome.total_testcases} test cases passed`}
        </Text>
      ) : null}
      {outcome.runtime_error ? (
        <Text variant="code" color="$red10" testID="ProblemEditorRunOutcomeRuntimeError">
          {outcome.runtime_error}
        </Text>
      ) : null}
      {outcome.compile_error ? (
        <Text variant="code" color="$red10" testID="ProblemEditorRunOutcomeCompileError">
          {outcome.compile_error}
        </Text>
      ) : null}
      {outcome.results?.length ? <RunCaseList results={outcome.results} /> : null}
    </Frame>
  );
}
