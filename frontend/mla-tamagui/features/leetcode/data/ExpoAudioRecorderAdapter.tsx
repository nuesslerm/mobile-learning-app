import { Logger } from '@lib/logging';
import {
  RecordingPresets,
  requestRecordingPermissionsAsync,
  setAudioModeAsync,
  useAudioRecorder,
  useAudioRecorderState,
} from 'expo-audio';
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  type ReactNode,
} from 'react';
import { Platform } from 'react-native';

import type { AudioRecordingPort, VoiceRecording } from '../domain/ports';

const METERING_UPDATE_INTERVAL_MS = 100;

// Empirically, ambient silence on a phone mic sits around -50dB or below and
// normal speaking volume reaches roughly -20dB to 0dB -- clamping to this
// range (rather than the raw -160..0 dBFS scale most APIs report) is what
// makes the waveform actually go flat during silence and react to real
// speech, instead of sitting at a barely-visible sliver the whole time.
const METERING_FLOOR_DB = -50;
const METERING_CEILING_DB = 0;

function meteringToAmplitude(db: number): number {
  const clamped = Math.max(METERING_FLOOR_DB, Math.min(METERING_CEILING_DB, db));
  return (clamped - METERING_FLOOR_DB) / (METERING_CEILING_DB - METERING_FLOOR_DB);
}

const RECORDING_OPTIONS = {
  ...RecordingPresets.HIGH_QUALITY,
  isMeteringEnabled: true,
};

type AudioState = { isRecording: boolean; durationMs: number; level: number };

const AudioRecordingContext = createContext<AudioRecordingPort | null>(null);

/**
 * Provider-as-adapter: owns the expo-audio recorder, its polling state,
 * permissions and audio mode (lifted from hooks/useVoiceRecording.ts) and
 * exposes the result as an AudioRecordingPort through context. Subscribers
 * get the same metering-derived level (0-1) the hook used to feed its
 * waveform.
 */
export function ExpoAudioRecorderProvider({ children }: { children: ReactNode }) {
  const recorder = useAudioRecorder(RECORDING_OPTIONS);
  const recorderState = useAudioRecorderState(
    recorder,
    METERING_UPDATE_INTERVAL_MS
  );
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const startedAtRef = useRef(0);
  const stateRef = useRef<AudioState>({ isRecording: false, durationMs: 0, level: 0 });
  const listenersRef = useRef<Set<(s: AudioState) => void>>(new Set());

  const notify = useCallback(() => {
    const snapshot = stateRef.current;
    for (const listener of listenersRef.current) listener(snapshot);
  }, []);

  const patch = useCallback(
    (p: Partial<AudioState>) => {
      stateRef.current = { ...stateRef.current, ...p };
      notify();
    },
    [notify]
  );

  // Metering arrives via useAudioRecorderState's own polling rather than an
  // event this adapter subscribes to directly, so react to its value changing.
  useEffect(() => {
    if (!stateRef.current.isRecording || recorderState.metering === undefined) return;
    patch({ level: meteringToAmplitude(recorderState.metering) });
  }, [recorderState.metering, patch]);

  const port = useMemo<AudioRecordingPort>(() => {
    const requestPermission = async (): Promise<boolean> => {
      const perm = await requestRecordingPermissionsAsync();
      Logger.withMetadata({ granted: perm.granted, canAskAgain: perm.canAskAgain }).info(
        'audio.recorder.permission'
      );
      return perm.granted;
    };

    const start = async (): Promise<void> => {
      const granted = await requestPermission();
      if (!granted) return;

      Logger.withMetadata({ platform: Platform.OS }).info('audio.recorder.start');
      await setAudioModeAsync({ allowsRecording: true, playsInSilentMode: true });
      await recorder.prepareToRecordAsync();
      recorder.record();
      startedAtRef.current = Date.now();
      patch({ isRecording: true, durationMs: 0, level: 0 });

      timerRef.current = setInterval(() => {
        patch({ durationMs: Date.now() - startedAtRef.current });
      }, 100);
    };

    const stop = async (): Promise<VoiceRecording | null> => {
      await recorder.stop();
      if (timerRef.current) clearInterval(timerRef.current);
      const uri = recorder.uri;
      const durationMs = Date.now() - startedAtRef.current;
      patch({ isRecording: false, durationMs: 0, level: 0 });

      if (!uri) {
        Logger.withMetadata({ durationMs }).warn('audio.recorder.stop_without_uri');
        return null;
      }
      Logger.withMetadata({ durationMs }).info('audio.recorder.stop');

      return {
        uri,
        // Matches RecordingPresets.HIGH_QUALITY's per-platform container:
        // '.m4a' (MPEG4AAC) on iOS/Android, 'audio/webm' on web -- sending
        // the wrong one doesn't break ffmpeg (it sniffs the real container
        // from content, not the extension), but it's misleading in logs and
        // wrong metadata is still wrong.
        mimeType:
          Platform.OS === 'web'
            ? 'audio/webm'
            : Platform.OS === 'ios'
              ? 'audio/m4a'
              : 'audio/mp4',
        durationMs: Date.now() - startedAtRef.current,
      };
    };

    const subscribe = (listener: (s: AudioState) => void): (() => void) => {
      listenersRef.current.add(listener);
      listener(stateRef.current);
      return () => {
        listenersRef.current.delete(listener);
      };
    };

    return { requestPermission, start, stop, subscribe };
  }, [recorder, patch]);

  return (
    <AudioRecordingContext.Provider value={port}>
      {children}
    </AudioRecordingContext.Provider>
  );
}

export function useExpoAudioRecordingPort(): AudioRecordingPort {
  const port = useContext(AudioRecordingContext);
  if (!port) {
    throw new Error('ExpoAudioRecorderProvider missing');
  }
  return port;
}