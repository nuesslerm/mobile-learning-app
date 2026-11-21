import { Text } from '@lib/tamagui-design-system';
import { YStack } from '@lib/tamagui-design-system/primitives';
import type { ReactNode } from 'react';
import type { LayoutChangeEvent } from 'react-native';

import { customOutcomeOf } from '../../domain/run/customOutcome';
import type { RunResponse } from '../../domain/run/execution';
import { CustomCaseResult } from './CustomCaseResult';
import { RunCaseList } from './RunCaseList';

/** What the card on screen shows: a run of the bundled cases, or of one case the learner typed (with the inputs it ran on). */
export type RunCard = { mode: 'tests'; response: RunResponse } | { mode: 'custom'; response: RunResponse; inputs: Record<string, unknown> };

type OnLayout = (event: LayoutChangeEvent) => void;

function Frame({ borderColor, onLayout, children }: { borderColor: '$green9' | '$red9' | '$borderColor'; onLayout?: OnLayout; children: ReactNode }) {
  return (
    <YStack mx="$3" mb="$1" px="$3" py="$2" rounded="$3" borderWidth={1} borderColor={borderColor} onLayout={onLayout} testID="ProblemEditorRunOutcome">
      {children}
    </YStack>
  );
}

function CustomCard({ response, inputs, onLayout }: { response: RunResponse; inputs: Record<string, unknown>; onLayout?: OnLayout }) {
  const outcome = customOutcomeOf(response, inputs);
  const failed = outcome.status === 'Error';
  return (
    <Frame borderColor={failed ? '$red9' : '$borderColor'} onLayout={onLayout}>
      <Text variant="strong" color={failed ? '$red10' : undefined} testID="ProblemEditorRunOutcomeStatus">
        {outcome.status}
      </Text>
      <CustomCaseResult outcome={outcome} />
    </Frame>
  );
}

/** The result of a run: for the bundled cases the status, totals, the failure text and every case; for a typed case what it returned. */
export function RunOutcomeCard({ card, onLayout }: { card: RunCard; onLayout?: OnLayout }) {
  if (card.mode === 'custom') return <CustomCard response={card.response} inputs={card.inputs} onLayout={onLayout} />;
  const outcome = card.response;
  const accepted = outcome.status_msg === 'Accepted';
  return (
    <Frame borderColor={accepted ? '$green9' : '$red9'} onLayout={onLayout}>
      <Text variant="strong" color={accepted ? '$green10' : '$red10'} testID="ProblemEditorRunOutcomeStatus">
        {outcome.status_msg}
      </Text>
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
