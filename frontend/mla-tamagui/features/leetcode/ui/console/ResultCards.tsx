import { Button, Text } from '@lib/tamagui-design-system';
import { FileText } from '@lib/tamagui-design-system/icons';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';

import { jsonText } from '../../domain/console/caseFields';
import type { CaseFocus, ConsoleResult, LastInput } from '../../domain/console/consoleResult';
import { ValueCard } from './ValueCard';

type Tone = '$success' | '$destructive' | undefined;

export type ResultActions = {
  marked: boolean;
  onShowLine: () => void;
  canOpenCase: (label: string) => boolean;
  onOpenCase: (label: string) => void;
};

const HEADLINES = { 'wrong-answer': 'Wrong Answer', 'runtime-error': 'Runtime Error', 'time-limit': 'Time Limit Exceeded', 'compile-error': 'Compile Error', accepted: 'Accepted', custom: 'Custom case', 'run-failed': 'Run failed' } as const;

const runtimeText = (ms: number) => `Runtime ${Math.round(ms)} ms`;

function Headline({ text, tone }: { text: string; tone: Tone }) {
  return (
    <Text variant="large" color={tone ?? '$color'} role="status" aria-live="polite" testID="ConsoleResultVerdict">
      {text}
    </Text>
  );
}

function Counts({ result }: { result: Extract<ConsoleResult, { counts: unknown }> }) {
  return result.counts ? (
    <Text variant="muted" testID="ConsoleResultCounts">
      {`${result.counts.correct} / ${result.counts.total} testcases passed`}
    </Text>
  ) : null;
}

function Inputs({ inputs, id }: { inputs: Record<string, unknown>; id: string }) {
  return (
    <>
      {Object.entries(inputs).map(([name, value]) => (
        <ValueCard key={name} id={`${id}-${name}`} caption={name} value={jsonText(value)} />
      ))}
    </>
  );
}

function Stdout({ text }: { text: string }) {
  return text ? <ValueCard id="ConsoleResultStdout" caption="Stdout" value={text} /> : null;
}

function Focus({ focus, wrong }: { focus: CaseFocus; wrong: boolean }) {
  return (
    <YStack gap="$3" testID="ConsoleResultFocus">
      <Inputs inputs={focus.inputs} id="ConsoleResultInput" />
      <ValueCard id="ConsoleResultOutput" caption="Output" value={jsonText(focus.actual)} tone={wrong ? '$red10' : undefined} />
      <ValueCard id="ConsoleResultExpected" caption="Expected" value={jsonText(focus.expected)} />
      <Stdout text={focus.stdout} />
    </YStack>
  );
}

function LastExecuted({ input, actions }: { input: LastInput; actions: ResultActions }) {
  return (
    <YStack gap="$2" testID="ConsoleResultLastInput">
      <XStack items="center" justify="space-between">
        <Text variant="muted">Last Executed Input</Text>
        {actions.canOpenCase(input.label) ? (
          <Button testID="ConsoleResultOpenCase" variant="link" size="$2" icon={FileText} pressLog="off" onPress={() => actions.onOpenCase(input.label)}>
            Open Testcase
          </Button>
        ) : null}
      </XStack>
      {Object.entries(input.inputs).map(([name, value]) => (
        <ValueCard key={name} id={`ConsoleResultLast-${name}`} caption={`${name} =`} value={jsonText(value)} />
      ))}
    </YStack>
  );
}

function ErrorBox({ message, line, fileName, actions }: { message: string; line: number | undefined; fileName: string; actions: ResultActions }) {
  const where = line !== undefined ? `Line ${line} (${fileName})` : null;
  return (
    <YStack gap="$2" px="$3" py="$2.5" rounded="$3" borderWidth={1} borderColor="$red9" role="alert" testID="ConsoleResultError">
      <Text variant="code" color="$red10" testID="ConsoleResultErrorText">
        {message}
      </Text>
      {where ? (
        actions.marked ? (
          <Button testID="ConsoleResultLine" variant="link" size="$2" pressLog="off" accessibilityLabel={`Show line ${line} in the editor`} onPress={actions.onShowLine}>
            {where}
          </Button>
        ) : (
          <Text variant="small" color="$red10" testID="ConsoleResultLine">
            {where}
          </Text>
        )
      ) : null}
    </YStack>
  );
}

/** The cards of one result: the headline and what its verdict shows. */
export function ResultCards({ result, actions }: { result: ConsoleResult; actions: ResultActions }) {
  const headline = HEADLINES[result.kind];
  switch (result.kind) {
    case 'accepted':
      return (
        <YStack gap="$3">
          <Headline text={headline} tone="$success" />
          <Counts result={result} />
          <Text variant="muted" testID="ConsoleResultRuntime">
            {runtimeText(result.runtimeMs)}
          </Text>
          {result.focus ? <Focus focus={result.focus} wrong={false} /> : null}
        </YStack>
      );
    case 'wrong-answer':
      return (
        <YStack gap="$3">
          <Headline text={headline} tone="$destructive" />
          <Counts result={result} />
          <Focus focus={result.focus} wrong />
        </YStack>
      );
    case 'runtime-error':
    case 'time-limit':
    case 'compile-error':
      return (
        <YStack gap="$3">
          <Headline text={headline} tone="$destructive" />
          <Counts result={result} />
          <ErrorBox message={result.message} line={result.errorAt?.line} fileName={result.fileName} actions={actions} />
          {result.lastInput ? <LastExecuted input={result.lastInput} actions={actions} /> : null}
          <Stdout text={result.stdout} />
        </YStack>
      );
    case 'custom':
      return (
        <YStack gap="$3">
          <Headline text={headline} tone={undefined} />
          <Inputs inputs={result.inputs} id="ConsoleResultInput" />
          <ValueCard id="ConsoleResultOutput" caption="Output" value={jsonText(result.result)} />
          <Stdout text={result.stdout} />
          <Text variant="muted" testID="ConsoleResultRuntime">
            {runtimeText(result.runtimeMs)}
          </Text>
        </YStack>
      );
    case 'run-failed':
      return (
        <YStack gap="$2">
          <Headline text={headline} tone="$destructive" />
          <Text variant="code" color="$red10" testID="ConsoleResultErrorText">
            {result.message}
          </Text>
          <Text variant="muted" testID="ConsoleResultHint">
            The code did not run. Try again.
          </Text>
        </YStack>
      );
  }
}
