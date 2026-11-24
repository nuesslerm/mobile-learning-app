import { Button, Icon, IconButton, MenuPanel, Text } from '@lib/tamagui-design-system';
import { Play, RotateCcw, Terminal } from '@lib/tamagui-design-system/icons';
import { XStack, YStack, useTheme } from '@lib/tamagui-design-system/primitives';
import { Platform } from 'react-native';

export type ConsoleActionsProps = {
  variant: 'bar' | 'footer';
  canvas?: boolean;
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
const CANVAS_PILL = { backgroundColor: '#101010', borderColor: '#252525', borderWidth: 1 } as const;
const FOOTER_PILL = { backgroundColor: '#29292c', borderColor: '#45454a', borderWidth: 1 } as const;
const SUBMIT_GLOW = { shadowColor: '#28dc57', shadowOpacity: 0.48, shadowRadius: 15, shadowOffset: { width: 0, height: 5 } } as const;
const CANVAS_CONTROL = { height: 42, fontSize: 17, color: '#f5f5f7' } as const;

function ConsoleReset({ footer, canvas, onReset }: { footer: boolean; canvas?: boolean; onReset: () => void }) {
  const theme = useTheme();
  const testID = footer ? 'ConsoleFooterReset' : 'ConsoleBarReset';
  if (ios) {
    return (
      <MenuPanel
        testID={testID}
        accessibilityLabel="Reset testcases"
        size={canvas ? 42 : 36}
        icon={RotateCcw}
        trigger={footer ? <Text variant="small" px="$2" fontSize={canvas ? 16 : undefined}>Reset</Text> : <Icon as={RotateCcw} size={canvas ? 19 : 14} color={canvas ? "#f5f5f7" : undefined} />}
        triggerStyle={canvas ? (footer ? { ...FOOTER_PILL, minWidth: 74 } : CANVAS_PILL) : { backgroundColor: theme.color4?.val }}
        placement={canvas && footer ? "beside" : "above"}
        glass={canvas && footer}
        panelWidth={canvas && footer ? 244 : 224}
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

export function ConsoleActions({ variant, canvas, onOpen, onReset, onRun, onSubmit, pending, noCases }: ConsoleActionsProps) {
  const prefix = variant === 'footer' ? 'ConsoleFooter' : 'ConsoleBar';
  const off = pending || noCases;
  const run = (
    <IconButton
      testID={`${prefix}Run`}
      icon={Play}
      iconSize={canvas ? 18 : iosIconSize}
      variant="secondary"
      size="$3"
      pressLog="off"
      disabled={off}
      accessibilityLabel="Run"
      onPress={onRun}
      {...(canvas ? { ...CANVAS_CONTROL, width: 42, style: variant === 'footer' ? FOOTER_PILL : CANVAS_PILL } : {})}
    />
  );
  const submit = (
    <Button
      testID={`${prefix}Submit`}
      variant="success"
      size="$3"
      rounded="$10"
      pressLog="off"
      disabled={off}
      accessibilityLabel="Submit"
      onPress={onSubmit}
      {...(canvas ? { ...CANVAS_CONTROL, minW: 84, style: { backgroundColor: '#1ec843', ...(variant === 'footer' ? SUBMIT_GLOW : {}) } } : {})}
    >
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
          <ConsoleReset footer canvas={canvas} onReset={onReset} />
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
      <Button
        testID="ConsoleBarOpen"
        variant="secondary"
        size="$3"
        rounded="$10"
        icon={Terminal}
        iconSize={canvas ? 18 : iosIconSize}
        pressLog="off"
        accessibilityLabel="Open console"
        onPress={onOpen}
        {...(canvas ? { ...CANVAS_CONTROL, minW: 122, style: CANVAS_PILL } : {})}
      >
        Console
      </Button>
      <XStack items="center" gap="$2">
        <ConsoleReset footer={false} canvas={canvas} onReset={onReset} />
        {run}
        {submit}
      </XStack>
    </XStack>
  );
}
