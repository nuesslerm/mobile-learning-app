import { YStack } from '@lib/tamagui-design-system/primitives';

import { showInput, showValue } from '../../domain/run/caseDisplay';
import type { CustomOutcome } from '../../domain/run/customOutcome';
import { Field } from './RunCaseList';

/** What one typed case did: the arguments, the value it returned, what it printed and raised, and how long it took. No verdict: nothing was compared. */
export function CustomCaseResult({ outcome }: { outcome: CustomOutcome }) {
  const id = 'ProblemEditorCustomCase';
  return (
    <YStack mt="$2" gap="$1" testID={id}>
      <Field id={`${id}-Input`} label="Input" value={showInput(outcome.inputs)} />
      <Field id={`${id}-Result`} label="Result" value={outcome.hasResult ? showValue(outcome.result) : '—'} />
      {outcome.stdout ? <Field id={`${id}-Stdout`} label="Output" value={outcome.stdout} /> : null}
      {outcome.error ? <Field id={`${id}-Error`} label="Error" value={outcome.error} tone="$red10" /> : null}
      {outcome.elapsedMs !== null ? <Field id={`${id}-Time`} label="Time" value={`${outcome.elapsedMs} ms`} /> : null}
    </YStack>
  );
}
