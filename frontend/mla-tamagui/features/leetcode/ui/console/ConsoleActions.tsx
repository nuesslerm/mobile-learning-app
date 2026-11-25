import { Button, Icon, IconButton, MenuPanel, Text } from '@lib/tamagui-design-system';
import { Play, RotateCcw, SquareTerminal, Terminal } from '@lib/tamagui-design-system/icons';
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
  noSelectedCase?: boolean;
};

/** The console's actions in two forms: the bar above the keyboard and the dock (Console, Reset, Run, Submit), the footer inside the sheet (a Reset pill, Run, Submit). */
const ios = Platform.OS === 'ios';
const iosIconSize = ios ? 14 : undefined;
const RESET_CONFIRM = 'ConsoleResetConfirmation';
const RESET_PATH = [RESET_CONFIRM];
const CANVAS_PILL = { backgroundColor: '#101010', borderColor: '#252525', borderWidth: 1 } as const;
const FOOTER_PILL = {
  backgroundColor: '#3a393d',
  backgroundImage: 'linear-gradient(180deg, #424145, #38373b 25%, #3f3e42 90%, #464549)',
  boxShadow: 'inset 0 1px 1px rgba(255,255,255,0.17), inset 0 -1px 1px rgba(255,255,255,0.11)',
} as const;
const FOOTER_SHADOW = { shadowColor: '#777479', shadowOpacity: 0.27, shadowRadius: 13, shadowOffset: { width: 6, height: 9 } } as const;
const SUBMIT_RIM = { borderColor: '#25dd4f', borderWidth: 1 } as const;
const FOOTER_SUBMIT = {
  backgroundColor: '#25c749',
  backgroundImage: 'linear-gradient(180deg, #2ddf57, #25c749 14%, #25c749 86%, #37e861)',
  boxShadow: 'inset 0 1px 1px rgba(130,255,150,0.42), inset 0 -1px 1px rgba(130,255,150,0.38)',
  shadowColor: '#28dc57',
  shadowOpacity: 0.58,
  shadowRadius: 15,
  shadowOffset: { width: 8, height: 7 },
} as const;
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
        trigger={footer ? <Text variant="small" px="$2" fontSize={canvas ? 16 : undefined}>Reset</Text> : <Icon as={RotateCcw} size={canvas ? 19 : 14} color={canvas ? "#f5f5f7" : undefined} systemSymbol={canvas ? 'arrow.counterclockwise' : undefined} />}
        triggerStyle={canvas ? (footer ? { ...FOOTER_PILL, ...FOOTER_SHADOW, minWidth: 76 } : CANVAS_PILL) : { backgroundColor: theme.color4?.val }}
        glassTrigger={canvas && !footer}
        placement={canvas && footer ? "beside" : "above"}
        glass={canvas}
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

export function ConsoleActions({ variant, canvas, onOpen, onReset, onRun, onSubmit, pending, noCases, noSelectedCase }: ConsoleActionsProps) {
  const footer = variant === 'footer';
  const prefix = footer ? 'ConsoleFooter' : 'ConsoleBar';
  const off = pending || noCases;
  const run = (
    <IconButton
      testID={`${prefix}Run`}
      icon={Play}
      iconSize={canvas ? 16 : iosIconSize}
      iconFilled={canvas}
      iconSystemSymbol={canvas ? 'play.fill' : undefined}
      variant="secondary"
      glass={canvas && !footer}
      size={canvas ? (footer ? 40 : 42) : "$3"}
      hitSlop={canvas && footer ? 2 : undefined}
      pressLog="off"
      disabled={off || noSelectedCase}
      accessibilityLabel="Run"
      onPress={onRun}
      style={canvas ? (footer ? { ...FOOTER_PILL, ...FOOTER_SHADOW } : CANVAS_PILL) : undefined}
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
      {...(canvas ? { ...CANVAS_CONTROL, ...(footer ? { height: 40, minW: 84, px: 11, hitSlop: 2 } : { minW: 86 }), fontWeight: "600", style: footer ? FOOTER_SUBMIT : { backgroundColor: '#1ec843', ...SUBMIT_RIM } } : {})}
    >
      Submit
    </Button>
  );
  const hint = noCases ? (
    <Text variant="muted" testID="ConsoleSubmitHint">
      This problem has no bundled cases to run.
    </Text>
  ) : null;
  if (footer) {
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
        glass={canvas}
        size="$3"
        rounded="$10"
        icon={canvas ? SquareTerminal : Terminal}
        iconSize={canvas ? 16 : iosIconSize}
        iconSystemSymbol={canvas ? { name: 'terminal', weight: 'medium' } : undefined}
        pressLog="off"
        accessibilityLabel="Open console"
        onPress={onOpen}
        {...(canvas ? { ...CANVAS_CONTROL, height: 44, fontSize: 17.5, minW: 128, style: CANVAS_PILL } : {})}
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
