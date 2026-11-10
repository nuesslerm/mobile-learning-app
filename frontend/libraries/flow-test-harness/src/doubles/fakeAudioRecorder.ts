// Structural mirror of features/leetcode/contracts/ports.ts `AudioRecordingPort` —
// the double stays import-free of mla-tamagui and is pure state/push logic.
type FakeAudioRecording = {
  uri: string;
  mimeType: string;
  durationMs: number;
};

type FakeAudioRecorderSnapshot = {
  isRecording: boolean;
  durationMs: number;
  level: number;
};

/** The three states a phone's microphone permission is in: asked and granted, asked and refused, or never asked. */
type FakeMicPermission = 'granted' | 'denied' | 'prompt';

export type FakeAudioRecorderConfig = {
  levels?: number[];
  durationMs?: number;
  /** `true` and `false` stay as they were (granted, denied). `'prompt'` has not been asked: the first request answers with `promptAnswer`. */
  permission?: boolean | FakeMicPermission;
  /** What the person taps when asked, in the `'prompt'` state. Default: allow. */
  promptAnswer?: boolean;
};

export type FakeAudioRecorder = {
  requestPermission(): Promise<boolean>;
  start(): Promise<void>;
  stop(): Promise<FakeAudioRecording | null>;
  subscribe(listener: (snapshot: FakeAudioRecorderSnapshot) => void): () => void;
  /** Pushes `{ isRecording, durationMs, level: n }` to every subscriber. */
  emitLevel(n: number): void;
  /** Where the permission stands now: it moves from `'prompt'` to granted or denied when requested. */
  readonly permission: FakeMicPermission;
  /** Method names called, oldest first. */
  calls: string[];
};

export function createFakeAudioRecorder(
  config: FakeAudioRecorderConfig = {}
): FakeAudioRecorder {
  const levels = config.levels ?? [];
  const durationMs = config.durationMs ?? 0;
  const initial = config.permission ?? true;
  let permission: FakeMicPermission = initial === true ? 'granted' : initial === false ? 'denied' : initial;

  const state = { isRecording: false, durationMs, level: 0 };
  const listeners = new Set<(s: FakeAudioRecorderSnapshot) => void>();
  const calls: string[] = [];

  const push = (): void => {
    const snapshot: FakeAudioRecorderSnapshot = { ...state };
    for (const listener of listeners) listener(snapshot);
  };

  return {
    async requestPermission(): Promise<boolean> {
      calls.push('requestPermission');
      if (permission === 'prompt') permission = config.promptAnswer === false ? 'denied' : 'granted';
      return permission === 'granted';
    },
    async start(): Promise<void> {
      calls.push('start');
      if (permission !== 'granted') {
        throw new Error(permission === 'prompt' ? 'Microphone permission was never requested' : 'Microphone permission denied');
      }
      state.isRecording = true;
      state.level = 0;
      push();
      for (const level of levels) {
        state.level = level;
        push();
      }
    },
    async stop(): Promise<FakeAudioRecording | null> {
      calls.push('stop');
      if (!state.isRecording) return null;
      state.isRecording = false;
      push();
      return {
        uri: 'fake://recording.m4a',
        mimeType: 'audio/m4a',
        durationMs: state.durationMs,
      };
    },
    subscribe(listener: (snapshot: FakeAudioRecorderSnapshot) => void): () => void {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    emitLevel(n: number): void {
      calls.push('emitLevel');
      state.level = n;
      push();
    },
    get permission(): FakeMicPermission {
      return permission;
    },
    get calls(): string[] {
      return [...calls];
    },
  };
}