import { useState } from 'react';
import { ActivityIndicator, Pressable, Text, View } from 'react-native';
import ChevronLeft from 'lucide-react-native/dist/esm/icons/chevron-left.mjs';
import Clipboard from 'lucide-react-native/dist/esm/icons/clipboard.mjs';
import QrCode from 'lucide-react-native/dist/esm/icons/qr-code.mjs';
import {
  useAgentComponents,
  type PairingScreenProps,
} from '@lib/agent-ui-contracts';
import { homePalettes, useOrcaScheme } from './appearance';
import { usePairingStyles } from './pairingStyles';

export function PairingScreen(props: PairingScreenProps) {
  const { PairingCodeSheet } = useAgentComponents();
  const colors = homePalettes[useOrcaScheme()];
  const styles = usePairingStyles();
  const [size, setSize] = useState(0);
  const intro = props.permission !== 'granted' && props.status === 'scanning';
  const denied = props.permission === 'denied';
  const pasteButton = (label: string) => (
    <Pressable
      accessibilityRole="button"
      onPress={props.onPaste}
      style={({ pressed }) => [
        styles.pasteButton,
        pressed && styles.pasteButtonPressed,
      ]}
    >
      <Clipboard size={16} color={colors.textSecondary} />
      <Text style={styles.pasteButtonText}>{label}</Text>
    </Pressable>
  );
  return (
    <View
      style={[
        styles.container,
        {
          paddingTop: props.topInset + 8,
          paddingBottom: props.bottomInset + 8,
        },
      ]}
    >
      <Pressable
        accessibilityLabel="Back"
        accessibilityRole="button"
        hitSlop={8}
        onPress={props.onBack}
        style={styles.backButton}
      >
        <ChevronLeft size={22} color={colors.textSecondary} />
      </Pressable>
      {props.permission === 'loading' ? (
        <ActivityIndicator color={colors.textSecondary} />
      ) : intro ? (
        <View style={styles.centered}>
          <Text style={styles.title}>
            {denied ? 'Camera Access Disabled' : 'Pair with desktop'}
          </Text>
          <Text style={styles.subtitle}>
            {denied
              ? 'Enable camera access in Settings, or paste the pairing code instead.'
              : 'Scan the QR code from Orca on your desktop, or paste the pairing code instead.'}
          </Text>
          <Pressable
            accessibilityRole="button"
            style={styles.primaryButton}
            onPress={props.onCamera}
          >
            {!denied && <QrCode size={16} color={colors.bgBase} />}
            <Text style={styles.primaryButtonText}>
              {denied ? 'Open Settings' : 'Continue'}
            </Text>
          </Pressable>
          {pasteButton('Paste code instead')}
        </View>
      ) : (
        <>
          <View style={styles.steps}>
            {[
              'Open Orca on your computer',
              'Go to Settings → Mobile',
              'Scan the QR code',
            ].map((step, i) => (
              <View key={step} style={styles.step}>
                <View style={styles.stepBadge}>
                  <Text style={styles.stepNumber}>{i + 1}</Text>
                </View>
                <Text style={styles.stepText}>{step}</Text>
              </View>
            ))}
          </View>
          {props.status === 'scanning' ? (
            <>
              <View
                style={
                  props.pasteVisible
                    ? styles.cameraPlaceholder
                    : styles.cameraWrap
                }
                onLayout={({ nativeEvent: { layout } }) =>
                  setSize(
                    Math.min(
                      360,
                      Math.round(Math.min(layout.width, layout.height) * 0.62)
                    )
                  )
                }
              >
                {!props.pasteVisible && (
                  <>
                    {props.camera}
                    <View pointerEvents="none" style={styles.reticle}>
                      <View
                        style={[
                          styles.reticleFrame,
                          { width: size, height: size },
                        ]}
                      >
                        {[
                          styles.cornerTL,
                          styles.cornerTR,
                          styles.cornerBL,
                          styles.cornerBR,
                        ].map((corner, i) => (
                          <View key={i} style={[styles.corner, corner]} />
                        ))}
                      </View>
                    </View>
                  </>
                )}
              </View>
              {pasteButton('Or paste pairing code')}
            </>
          ) : (
            <View style={styles.centered}>
              {props.status === 'connecting' ? (
                <>
                  <ActivityIndicator color={colors.textSecondary} />
                  <Text style={styles.connectingText}>Connecting…</Text>
                </>
              ) : (
                <>
                  <Text accessibilityRole="alert" style={styles.errorText}>
                    {props.error}
                  </Text>
                  <View style={styles.errorActions}>
                    <Pressable
                      accessibilityRole="button"
                      style={styles.primaryButton}
                      onPress={props.onRetry}
                    >
                      <Text style={styles.primaryButtonText}>Try Again</Text>
                    </Pressable>
                    <Pressable
                      accessibilityRole="button"
                      style={styles.secondaryButton}
                      onPress={props.onPaste}
                    >
                      <Text style={styles.secondaryButtonText}>
                        Paste code instead
                      </Text>
                    </Pressable>
                  </View>
                </>
              )}
            </View>
          )}
        </>
      )}
      <PairingCodeSheet
        visible={props.pasteVisible}
        onCancel={props.onCancel}
        onSubmit={props.onSubmit}
        bottomInset={props.bottomInset} keyboardInset={props.keyboardInset}
      />
    </View>
  );
}
