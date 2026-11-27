import {
  useSkin,
  type NotificationChoicesProps,
  type OnboardingChoiceProps,
  type PairDesktopButtonProps,
  type PairedDesktopsProps,
  type PairingCodeSheetProps,
  type PairingScreenProps,
  type UnpairedHomeProps,
} from '@lib/skin';
import { Button, Icon, InsetGroup, ListItem, ScreenHeader, Spinner, Text, TextField } from '@lib/tamagui-design-system';
import { Clipboard, Monitor, Plus, QrCode } from '@lib/tamagui-design-system/icons';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';
import type { LucideIcon } from 'lucide-react-native';
import { useState } from 'react';
import { ScrollView } from 'react-native';

import { ScreenSheet } from '../sheet/ScreenSheet';

export function OnboardingChoice(props: OnboardingChoiceProps) {
  return (
    <Button
      variant={props.primary ? 'default' : 'ghost'}
      // a plain choice is text, not a glass capsule
      glass={Boolean(props.primary)}
      accessibilityLabel={props.accessibilityLabel}
      disabled={props.disabled}
      pressLog="off"
      onPress={props.onPress}
      testID="skin-onboarding-choice"
    >
      {props.busy ? <Spinner colorToken={props.primary ? 'color1' : 'color11'} /> : props.label}
    </Button>
  );
}

export function NotificationChoices(props: NotificationChoicesProps) {
  // taken from the skin, so a replaced choice replaces both of these
  const { OnboardingChoice: Choice } = useSkin();
  return (
    <YStack gap="$2">
      <Choice
        label={props.enableLabel}
        accessibilityLabel={props.enableAccessibilityLabel}
        primary
        busy={props.busyChoice === 'enable'}
        disabled={props.disabled}
        onPress={() => props.onChoice('enable')}
      />
      <Choice
        label={props.skipLabel}
        accessibilityLabel={props.skipAccessibilityLabel}
        busy={props.busyChoice === 'skip'}
        disabled={props.disabled}
        onPress={() => props.onChoice('skip')}
      />
    </YStack>
  );
}

export function PairDesktopButton(props: PairDesktopButtonProps) {
  return (
    <Button self="center" pressLog="off" onPress={props.onPress} testID="skin-pair-desktop">
      <OnAccentIcon as={QrCode} />
      {props.label}
    </Button>
  );
}

// the button's own icon slot draws in the page's ink; on the filled button the glyph takes the label's colour
function OnAccentIcon({ as }: { as: LucideIcon }) {
  return <Icon as={as} size={18} color="$color1" />;
}

function StepNumber({ value }: { value: number }) {
  return (
    <YStack width={28} height={28} rounded={14} items="center" justify="center" bg="$color4">
      <Text variant="small">{value}</Text>
    </YStack>
  );
}

export function UnpairedHome(props: UnpairedHomeProps) {
  const { BrandMark } = useSkin();
  return (
    <ScrollView
      contentContainerStyle={[
        { paddingTop: 24, paddingBottom: props.bottomInset + 24, gap: 24 },
        props.isWideLayout && { maxWidth: props.contentMaxWidth, width: '100%', alignSelf: 'center' },
      ]}
    >
      <YStack items="center" gap="$3" px="$6">
        <BrandMark size={44} />
        <Text variant="h3" role="heading" text="center">
          {props.title}
        </Text>
        <Text variant="lead" text="center">
          {props.description}
        </Text>
      </YStack>
      <InsetGroup testID="skin-unpaired-steps" separatorInset={56}>
        {props.steps.map((step, index) => (
          <ListItem key={step.id} variant="grouped" testID="skin-unpaired-step">
            <XStack gap="$3">
              <StepNumber value={index + 1} />
              <YStack flex={1} gap="$1">
                <Text variant="strong">{step.title}</Text>
                <Text variant="muted">{step.description}</Text>
              </YStack>
            </XStack>
          </ListItem>
        ))}
      </InsetGroup>
      <PairDesktopButton label={props.pairLabel} onPress={props.onPairDesktop} />
    </ScrollView>
  );
}

/** The paired desktop and the way to pair another; its screen pads the sides. */
export function PairedDesktops({ desktop, onPair, onOpen }: PairedDesktopsProps) {
  return (
    <YStack gap="$2" py="$3">
      <Text variant="muted" role="heading" textTransform="uppercase" letterSpacing={0.5}>
        Desktops
      </Text>
      <InsetGroup inset={false} testID="skin-desktops">
        {desktop ? (
          <ListItem
            variant="grouped"
            showChevron
            pressLog="off"
            accessibilityLabel={`Open ${desktop.name}`}
            onPress={onOpen}
            testID="skin-desktop"
          >
            <XStack items="center" gap="$3">
              <YStack>
                <Icon as={Monitor} size={20} />
                <YStack
                  position="absolute"
                  r={-3}
                  b={-3}
                  width={9}
                  height={9}
                  rounded={5}
                  bg={desktop.offline ? '$yellow9' : '$green9'}
                />
              </YStack>
              <YStack flex={1} gap="$1">
                <Text numberOfLines={1}>{desktop.name}</Text>
                <Text variant="muted" numberOfLines={1}>
                  {desktop.summary}
                </Text>
              </YStack>
            </XStack>
          </ListItem>
        ) : null}
        <ListItem variant="grouped" pressLog="off" accessibilityLabel="Pair a desktop" onPress={onPair} testID="skin-desktops-pair">
          <XStack items="center" gap="$3">
            <Icon as={Plus} size={18} color="$blue10" />
            <Text color="$blue10">Pair a desktop</Text>
          </XStack>
        </ListItem>
      </InsetGroup>
    </YStack>
  );
}

const STEPS = ['Open Orca on your computer', 'Go to Settings → Mobile', 'Scan the QR code'];

export function PairingScreen(props: PairingScreenProps) {
  const [size, setSize] = useState(0);
  const intro = props.permission !== 'granted' && props.status === 'scanning';
  const denied = props.permission === 'denied';
  const paste = (label: string) => (
    <Button variant="ghost" glass={false} self="center" icon={Clipboard} pressLog="off" onPress={props.onPaste} testID="skin-pairing-paste">
      {label}
    </Button>
  );
  return (
    <YStack flex={1} bg="$background" pb={props.bottomInset + 8}>
      <ScreenHeader testID="skin-pairing-header" title="Pairing" onBack={props.onBack} backAccessibilityLabel="Back" />
      {props.permission === 'loading' ? (
        <YStack flex={1} items="center" justify="center">
          <Spinner />
        </YStack>
      ) : intro ? (
        <YStack flex={1} items="center" justify="center" gap="$3" px="$6">
          <Text variant="h4" text="center">
            {denied ? 'Camera Access Disabled' : 'Pair with desktop'}
          </Text>
          <Text variant="muted" text="center">
            {denied
              ? 'Enable camera access in Settings, or paste the pairing code instead.'
              : 'Scan the QR code from Orca on your desktop, or paste the pairing code instead.'}
          </Text>
          <Button pressLog="off" onPress={props.onCamera} testID="skin-pairing-camera">
            {denied ? null : <OnAccentIcon as={QrCode} />}
            {denied ? 'Open Settings' : 'Continue'}
          </Button>
          {paste('Paste code instead')}
        </YStack>
      ) : (
        <>
          <YStack pt="$4">
            <InsetGroup testID="skin-pairing-steps" separatorInset={56}>
              {STEPS.map((step, index) => (
                <ListItem key={step} variant="grouped" testID="skin-pairing-step">
                  <XStack items="center" gap="$3">
                    <StepNumber value={index + 1} />
                    <Text flex={1}>{step}</Text>
                  </XStack>
                </ListItem>
              ))}
            </InsetGroup>
          </YStack>
          {props.status === 'scanning' ? (
            <>
              <YStack
                flex={1}
                m="$4"
                rounded="$4"
                overflow="hidden"
                bg="$color3"
                onLayout={({ nativeEvent: { layout } }) =>
                  setSize(Math.min(360, Math.round(Math.min(layout.width, layout.height) * 0.62)))
                }
              >
                {props.pasteVisible ? null : (
                  <>
                    {props.camera}
                    <YStack position="absolute" t={0} r={0} b={0} l={0} items="center" justify="center" pointerEvents="none">
                      <YStack width={size} height={size} rounded="$4" borderWidth={2} borderColor="white" opacity={0.9} />
                    </YStack>
                  </>
                )}
              </YStack>
              {paste('Or paste pairing code')}
            </>
          ) : (
            <YStack flex={1} items="center" justify="center" gap="$3" px="$6">
              {props.status === 'connecting' ? (
                <>
                  <Spinner />
                  <Text variant="muted">Connecting…</Text>
                </>
              ) : (
                <>
                  <Text role="alert" color="$red10" text="center">
                    {props.error}
                  </Text>
                  <Button pressLog="off" onPress={props.onRetry} testID="skin-pairing-retry">
                    Try Again
                  </Button>
                  <Button variant="ghost" glass={false} pressLog="off" onPress={props.onPaste} testID="skin-pairing-paste-again">
                    Paste code instead
                  </Button>
                </>
              )}
            </YStack>
          )}
        </>
      )}
      <PairingCodeSheet
        visible={props.pasteVisible}
        onCancel={props.onCancel}
        onSubmit={props.onSubmit}
        bottomInset={props.bottomInset}
        keyboardInset={props.keyboardInset}
      />
    </YStack>
  );
}

export function PairingCodeSheet({ visible, ...props }: PairingCodeSheetProps) {
  return visible ? <CodeEntry {...props} /> : null;
}

function CodeEntry(props: Omit<PairingCodeSheetProps, 'visible'>) {
  const [text, setText] = useState('');
  const code = text.trim();
  const save = () => {
    if (code) props.onSubmit(code);
  };
  return (
    <ScreenSheet onClose={props.onCancel} handleLabel="Close" testID="skin-pairing-code">
      <YStack gap="$3" px="$4" pb={Math.max(props.bottomInset, 16)}>
        <YStack gap="$1">
          <Text variant="large">Paste pairing code</Text>
          <Text variant="muted">Copy the code shown under the QR on your computer.</Text>
        </YStack>
        <TextField
          testID="MobilePairCode"
          aria-label="Pairing code"
          autoFocus
          value={text}
          onChangeText={setText}
          autoCapitalize="none"
          autoCorrect={false}
          returnKeyType="done"
          onSubmitEditing={save}
          placeholder="orca://pair?code=... or paste the code"
          placeholderTextColor="$color10"
        />
        <XStack gap="$3" justify="flex-end">
          <Button variant="ghost" glass={false} pressLog="off" onPress={props.onCancel} testID="skin-pairing-code-cancel">
            Cancel
          </Button>
          <Button disabled={!code} pressLog="off" onPress={save} testID="skin-pairing-code-save">
            Save
          </Button>
        </XStack>
      </YStack>
    </ScreenSheet>
  );
}
