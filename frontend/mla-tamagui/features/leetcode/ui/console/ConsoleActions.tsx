import { Button, Icon, IconButton, MenuPanel, Text } from '@lib/tamagui-design-system';
import { Play, RotateCcw, Terminal } from '@lib/tamagui-design-system/icons';
import { XStack, YStack, useTheme } from '@lib/tamagui-design-system/primitives';
import { Platform } from 'react-native';

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
const ios = Platform.OS === 'ios';
const iosIconSize = ios ? 14 : undefined;
const RESET_CONFIRM = 'ConsoleResetConfirmation';
const RESET_PATH = [RESET_CONFIRM];

function ConsoleReset({ footer, onReset }: { footer: boolean; onReset: () => void }) {
  const theme = useTheme();
  const testID = footer ? 'ConsoleFooterReset' : 'ConsoleBarReset';
  if (ios) {
    return (
      <MenuPanel
        testID={testID}
        accessibilityLabel="Reset testcases"
        size={36}
        icon={RotateCcw}
        trigger={footer ? <Text variant="small" px="$2">Reset</Text> : <Icon as={RotateCcw} size={14} />}
        triggerStyle={{ backgroundColor: theme.color4?.val }}
        placement="above"
        panelWidth={224}
        openPath={RESET_PATH}
        items={[{
          kind: 'confirm',
          testID: RESET_CONFIRM,
          label: 'Reset',
          title: 'Reset to sample test cases',
          message: '',
          confirmLabel: 'Reset',
          onConfirm: onReset,
        }]}
      />
    );
  }
  return footer ? (
    <Button testID={testID} variant="secondary" size="$3" rounded="$10" pressLog="off" accessibilityLabel="Reset testcases" onPress={onReset}>
      Reset
    </Button>
  ) : (
    <IconButton testID={testID} icon={RotateCcw} variant="secondary" size="$3" pressLog="off" accessibilityLabel="Reset testcases" onPress={onReset} />
  );
}

export function ConsoleActions({ variant, onOpen, onReset, onRun, onSubmit, pending, noCases }: ConsoleActionsProps) {
  const prefix = variant === 'footer' ? 'ConsoleFooter' : 'ConsoleBar';
  const off = pending || noCases;
  const run = (
    <IconButton testID={`${prefix}Run`} icon={Play} iconSize={iosIconSize} variant="secondary" size="$3" pressLog="off" disabled={off} accessibilityLabel="Run" onPress={onRun} />
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
          <ConsoleReset footer onReset={onReset} />
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
      <Button testID="ConsoleBarOpen" variant="secondary" size="$3" rounded="$10" icon={Terminal} iconSize={iosIconSize} pressLog="off" accessibilityLabel="Open console" onPress={onOpen}>
        Console
      </Button>
      <XStack items="center" gap="$2">
        <ConsoleReset footer={false} onReset={onReset} />
        {run}
        {submit}
      </XStack>
    </XStack>
  );
}
