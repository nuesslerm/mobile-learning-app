import { useEffect, useRef, useState } from 'react';
import { AppState, Linking, StyleSheet } from 'react-native';
import { useKeyboardFrame } from '@lib/keyboard';
import { CameraView } from 'expo-camera';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import {
  useSkin,
  type PairingScreenProps,
} from '@lib/skin';
import type { PairingCameraPermission } from '../domain/ports';
import { useCodingAgentPorts } from '../state/ports';
import { parseT3PairingInvite } from '../domain/t3PairingInvite';

interface Props {
  onBack(): void;
  onPair(endpoint: string, code: string, signal: AbortSignal): Promise<void>;
}

export function T3PairingScreen({ onBack, onPair }: Props) {
  const { PairingScreen } = useSkin();
  const { pairingCamera } = useCodingAgentPorts();
  const [permission, setPermission] = useState<PairingCameraPermission>();

  const [pasteVisible, setPasteVisible] = useState(false);
  const [status, setStatus] =
    useState<PairingScreenProps['status']>('scanning');
  const [error, setError] = useState('');
  const active = useRef<AbortController | null>(null);
  const keyboard = useKeyboardFrame();
  const { top, bottom } = useSafeAreaInsets();
  useEffect(() => {
    let mounted = true;
    const refresh = async () => {
      const next = await pairingCamera.get().catch(() => ({granted: false, canAskAgain: true}));
      if (mounted) setPermission(next);
    };
    void refresh();
    const subscription = AppState.addEventListener('change', (state) => {
      if (state === 'active') void refresh();
    });
    return () => {
      mounted = false;
      subscription.remove();
      active.current?.abort();
      active.current = null;
    };
  }, [pairingCamera]);

  async function submit(value: string) {
    setPasteVisible(false);
    if (active.current) return;
    const invite = parseT3PairingInvite(value);
    if (!invite) {
      setError(
        'Not a valid pairing code — copy it from your computer and paste again'
      );
      setStatus('error');
      return;
    }
    const attempt = new AbortController();
    active.current = attempt;
    setStatus('connecting');
    const timeout = setTimeout(() => attempt.abort(), 25_000);
    try {
      await onPair(invite.endpoint, invite.code, attempt.signal);
    } catch (cause) {
      if (active.current !== attempt) return;
      setError(
        attempt.signal.aborted
          ? 'The connection timed out. Check that the desktop is online and try again.'
          : cause instanceof Error
            ? cause.message
            : 'Pairing failed.'
      );
      setStatus('error');
    } finally {
      clearTimeout(timeout);
      if (active.current === attempt) active.current = null;
    }
  }
  return (
    <PairingScreen
      permission={
        !permission
          ? 'loading'
          : permission.granted
            ? 'granted'
            : permission.canAskAgain === false
              ? 'denied'
              : 'prompt'
      }
      status={status}
      error={error}
      topInset={top}
      bottomInset={bottom} keyboardInset={keyboard.height}
      pasteVisible={pasteVisible}
      camera={
        permission?.granted && !pasteVisible && status === 'scanning' ? (
          <CameraView
            facing="back"
            style={StyleSheet.absoluteFill}
            barcodeScannerSettings={{ barcodeTypes: ['qr'] }}
            onBarcodeScanned={({ data }) => void submit(data)}
          />
        ) : undefined
      }
      onBack={onBack}
      onCamera={() => {
        const action =
          permission?.canAskAgain === false
            ? Linking.openSettings()
            : pairingCamera.request().then(setPermission);
        void action.catch(() => {
          setError(
            'Camera access is unavailable. Paste the pairing code instead.'
          );
          setStatus('error');
        });
      }}
      onPaste={() => {
        setStatus('scanning');
        setPasteVisible(true);
      }}
      onCancel={() => setPasteVisible(false)}
      onRetry={() => setStatus('scanning')}
      onSubmit={(value) => void submit(value)}
    />
  );
}
