import { Text } from '@lib/tamagui-design-system';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';
import { useState } from 'react';
import { Pressable } from 'react-native';

import { clip, orderCases, showInput, showValue } from '../../domain/run/caseDisplay';
import type { TestResult } from '../../domain/run/execution';

export function Field({ id, label, value, tone }: { id: string; label: string; value: string; tone?: '$red10' }) {
  const [open, setOpen] = useState(false);
  const shown = open ? { text: value, clipped: true } : clip(value);
  return (
    <Pressable disabled={!shown.clipped} onPress={() => setOpen((was) => !was)} accessibilityRole={shown.clipped ? 'button' : undefined} testID={`${id}-Toggle`}>
      <XStack gap="$2">
        <Text variant="muted" width={64}>
          {label}
        </Text>
        <Text variant="code" flex={1} color={tone} testID={id}>
          {shown.text}
        </Text>
      </XStack>
    </Pressable>
  );
}

function Case({ index, result }: { index: number; result: TestResult }) {
  const id = `ProblemEditorRunCase-${index}`;
  return (
    <YStack gap="$1" py="$2" borderTopWidth={1} borderColor="$borderColor" testID={id}>
      <XStack gap="$2" items="center">
        <Text variant="strong" color={result.pass ? '$green10' : '$red10'} testID={`${id}-Verdict`}>
          {`Case ${index + 1} · ${result.pass ? 'passed' : 'failed'}`}
        </Text>
        <Text variant="muted" testID={`${id}-Time`}>
          {`${result.elapsed_ms} ms`}
        </Text>
      </XStack>
      <Field id={`${id}-Input`} label="Input" value={showInput(result.input)} />
      <Field id={`${id}-Expected`} label="Expected" value={showValue(result.expected)} />
      <Field id={`${id}-Actual`} label="Actual" value={result.error ? '—' : showValue(result.actual)} tone={result.pass ? undefined : '$red10'} />
      {result.error ? <Field id={`${id}-Error`} label="Error" value={result.error} tone="$red10" /> : null}
      {result.stdout ? <Field id={`${id}-Stdout`} label="Output" value={result.stdout} /> : null}
    </YStack>
  );
}

/** Every case of a run: the failing ones first, long values clipped to a line and opened by a tap. */
export function RunCaseList({ results }: { results: readonly TestResult[] }) {
  return (
    <YStack mt="$2" testID="ProblemEditorRunCases">
      {orderCases(results).map(({ index, result }) => (
        <Case key={index} index={index} result={result} />
      ))}
    </YStack>
  );
}
