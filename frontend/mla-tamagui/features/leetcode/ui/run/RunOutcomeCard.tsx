import { Text } from '@lib/tamagui-design-system';
import { YStack } from '@lib/tamagui-design-system/primitives';
import type { LayoutChangeEvent } from 'react-native';

import type { RunResponse } from '../../domain/run/execution';
import { RunCaseList } from './RunCaseList';

/** What the card on screen shows: a run of the bundled cases, or of one case the learner typed (with the inputs it ran on). */
export type RunCard = { mode: 'tests'; response: RunResponse } | { mode: 'custom'; response: RunResponse; inputs: Record<string, unknown> };

/** The result of Run Local: status, totals, the failure text and every case. */
export function RunOutcomeCard({ card, onLayout }: { card: RunCard; onLayout?: (event: LayoutChangeEvent) => void }) {
  const outcome = card.response;
  const accepted = outcome.status_msg === 'Accepted';
  return (
    <YStack mx="$3" mb="$1" px="$3" py="$2" rounded="$3" borderWidth={1} borderColor={accepted ? '$green9' : '$red9'} onLayout={onLayout} testID="ProblemEditorRunOutcome">
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
    </YStack>
  );
}
