import { Logger } from '@lib/logging';
import { useCallback, useEffect, useRef, useState } from 'react';

import type { VoiceRecording } from '../domain/ports';
import { useLeetCodePorts } from './ports';

const BAR_COUNT = 14;

export type { VoiceRecording };

export interface UseVoiceRecordingResult {
  isRecording: boolean;
  durationMs: number;
  /**
   * Rolling window of the last BAR_COUNT real mic-level samples (0-1, oldest
   * first) -- drives the waveform so it only wobbles while actual speech is
   * being picked up, rather than animating on a fixed timer regardless of
   * whether anyone is talking. Empty while not recording.
   */
  amplitudes: number[];
  startRecording: () => Promise<void>;
  stopRecording: () => Promise<VoiceRecording | null>;
}

export function useVoiceRecording(): UseVoiceRecordingResult {
  const { audio } = useLeetCodePorts();
  const [isRecording, setIsRecording] = useState(false);
  const [durationMs, setDurationMs] = useState(0);
  const [amplitudes, setAmplitudes] = useState<number[]>([]);
  const ampBufferRef = useRef<number[]>([]);

  // The audio port pushes a snapshot on every change (lifecycle, duration,
  // metering level). While recording, fold each level into the rolling
  // waveform window; while silent, leave it empty.
  useEffect(() => {
    return audio.subscribe((s) => {
      setIsRecording(s.isRecording);
      setDurationMs(s.durationMs);
      if (s.isRecording) {
        const next = [...ampBufferRef.current, s.level].slice(-BAR_COUNT);
        ampBufferRef.current = next;
        setAmplitudes(next);
      }
    });
  }, [audio]);

  const startRecording = useCallback(async () => {
    const granted = await audio.requestPermission();
    if (!granted) {
      Logger.warn('voice_edit.record.permission_denied');
      return;
    }

    ampBufferRef.current = [];
    setAmplitudes([]);
    setDurationMs(0);
    try {
      await audio.start();
    } catch (err) {
      Logger.withError(err).error('voice_edit.record.start_failed');
      throw err;
    }
  }, [audio]);

  const stopRecording = useCallback(async (): Promise<VoiceRecording | null> => {
    ampBufferRef.current = [];
    setAmplitudes([]);
    try {
      return await audio.stop();
    } catch (err) {
      Logger.withError(err).error('voice_edit.record.stop_failed');
      throw err;
    }
  }, [audio]);

  return {
    isRecording,
    durationMs,
    amplitudes,
    startRecording,
    stopRecording,
  };
}