import { Button, IconButton, Text } from '@lib/tamagui-design-system';
import { Play, RotateCcw, Terminal } from '@lib/tamagui-design-system/icons';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';

export type ConsoleActionsProps = {
  variant: 'bar' | 'footer';
  onOpen?: () => void;
  onReset: () => void;
  onRun: () => void;
  onSubmit: () => void;
  /** A run is in flight: Run and Submit are off until it answers. */
  pending: boolean;
  /** The problem has no bundled cases: there is nothing to run or submit. */
  noCases: boolean;
};

/** The console's actions in two forms: the bar above the keyboard and the dock (Console, Reset, Run, Submit), the footer inside the sheet (a Reset pill, Run, Submit). */
export function ConsoleActions({ variant, onOpen, onReset, onRun, onSubmit, pending, noCases }: ConsoleActionsProps) {
  const prefix = variant === 'footer' ? 'ConsoleFooter' : 'ConsoleBar';
  const off = pending || noCases;
  const run = (
    <IconButton testID={`${prefix}Run`} icon={Play} variant="secondary" size="$3" pressLog="off" disabled={off} accessibilityLabel="Run" onPress={onRun} />
  );
  const submit = (
    <Button testID={`${prefix}Submit`} variant="success" size="$3" rounded="$10" pressLog="off" disabled={off} accessibilityLabel="Submit" onPress={onSubmit}>
      Submit
    </Button>
  );
  const hint = noCases ? (
    <Text variant="muted" testID="ConsoleSubmitHint">
      This problem has no bundled cases to run.
    </Text>
  ) : null;
  if (variant === 'footer') {
    return (
      <YStack gap="$1.5" testID="ConsoleFooter">
        <XStack items="center" justify="space-between" gap="$2">
          <Button testID="ConsoleFooterReset" variant="secondary" size="$3" rounded="$10" pressLog="off" accessibilityLabel="Reset testcases" onPress={onReset}>
            Reset
          </Button>
          <XStack items="center" gap="$2">
            {run}
            {submit}
          </XStack>
        </XStack>
        {hint}
      </YStack>
    );
  }
  return (
    <XStack items="center" justify="space-between" gap="$2" testID="ConsoleBar">
      <Button testID="ConsoleBarOpen" variant="secondary" size="$3" rounded="$10" icon={Terminal} pressLog="off" accessibilityLabel="Open console" onPress={onOpen}>
        Console
      </Button>
      <XStack items="center" gap="$2">
        <IconButton testID="ConsoleBarReset" icon={RotateCcw} variant="secondary" size="$3" pressLog="off" accessibilityLabel="Reset testcases" onPress={onReset} />
        {run}
        {submit}
      </XStack>
    </XStack>
  );
}
