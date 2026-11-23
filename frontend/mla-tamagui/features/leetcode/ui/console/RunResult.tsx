import { Text } from '@lib/tamagui-design-system';

import type { ConsoleOutcome } from '../../state/console/useConsole';
import { ResultCards, type ResultActions } from './ResultCards';

/** The Run Result tab: the outcome of the last run, or an empty state. */
export function RunResult({ outcome, actions }: { outcome: ConsoleOutcome | null; actions: Omit<ResultActions, 'marked'> }) {
  if (!outcome) {
    return (
      <Text variant="muted" testID="ConsoleResultEmpty">
        Run your code to see the result here
      </Text>
    );
  }
  return <ResultCards result={outcome.result} actions={{ ...actions, marked: outcome.marked }} />;
}
