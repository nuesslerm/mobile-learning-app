import type { RunResponse } from '../domain';
import type {
  EditAlternative,
  VoiceEditResponse,
} from '@lib/voice-edit-dsl';

import type {
  AudioRecordingPort,
  LeetCodeGatewayPort,
  LeetCodePorts,
  VoiceEditTransportPort,
} from '../contracts/ports';

// Two Sum starter code (TypeScript) the plan alternatives edit.
const TWO_SUM_RUN: RunResponse = {
  source: 'backend-run',
  kind: 'test',
  state: 'SUCCESS',
  requestId: 'fixture',
  run_success: true,
  status_msg: 'Accepted',
  status_runtime: '42 ms',
  total_correct: 12,
  total_testcases: 12,
  results: Array.from({ length: 12 }, () => ({
    pass: true,
    input: { nums: [2, 7, 11, 15], target: 9 },
    actual: [0, 1],
    expected: [0, 1],
    stdout: '',
    stderr: '',
    error: null,
    elapsed_ms: 42,
  })),
};

const MINIMAL_RUN: RunResponse = {
  source: 'backend-run',
  kind: 'test',
  state: 'SUCCESS',
  requestId: '',
  run_success: true,
  status_msg: 'Accepted',
  total_correct: 0,
  total_testcases: 0,
};

const FIXTURE_TRANSCRIPT =
  'rename nums to values and return early when target is found';

// Two alternatives against the Two Sum JavaScript starter (the class-based
// catalog starter, lines 1-10): a plain hash-map pass, then a second that
// renames the `nums` parameter to `values`. Each command writes at most the
// validator's 6-line cap so the picker accepts them.
const PLAN_ALTERNATIVES: EditAlternative[] = [
  {
    id: 'hash-map-pass',
    label: 'Hash map pass',
    summary:
      'Build a value-to-index map and return as soon as the complement is found',
    confidence: 0.94,
    commands: [
      {
        type: 'replaceLines',
        startLine: 8,
        endLine: 8,
        newText: [
          '        const seen = new Map<number, number>();',
          '        for (let i = 0; i < nums.length; i++) {',
          '            const need = target - nums[i];',
          '            if (seen.has(need)) return [seen.get(need)!, i];',
          '            seen.set(nums[i], i);',
          '        }',
        ].join('\n'),
      },
    ],
  },
  {
    id: 'rename-to-values',
    label: 'Rename nums to values',
    summary:
      'Rename the parameter to values and solve with a hash map, returning early',
    confidence: 0.87,
    commands: [
      {
        type: 'replaceLines',
        startLine: 7,
        endLine: 7,
        newText: '    twoSum(values, target) {',
      },
      {
        type: 'replaceLines',
        startLine: 8,
        endLine: 8,
        newText: [
          '        const seen = new Map<number, number>();',
          '        for (let i = 0; i < values.length; i++) {',
          '            const need = target - values[i];',
          '            if (seen.has(need)) return [seen.get(need)!, i];',
          '            seen.set(values[i], i);',
          '        }',
        ].join('\n'),
      },
    ],
  },
];

// 24 sine-like levels, one full cycle 0..1..0. The pinned recording's
// durationMs (2400) stands alone -- it is not 24 x 80ms.
const FIXTURE_LEVELS = [
  0.5, 0.62, 0.73, 0.82, 0.9, 0.95, 0.98, 0.95, 0.9, 0.82, 0.73, 0.62,
  0.5, 0.38, 0.27, 0.18, 0.1, 0.05, 0.02, 0.05, 0.1, 0.18, 0.27, 0.38,
];

const LEVEL_INTERVAL_MS = 80;

const FIXTURE_RECORDING = {
  uri: 'fixture://take.m4a',
  mimeType: 'audio/m4a',
  durationMs: 2400,
};

type AudioLevel = { isRecording: boolean; durationMs: number; level: number };

function createFixtureGateway(): LeetCodeGatewayPort {
  return {
    userStatus: async () => ({ isSignedIn: true, username: 'fixture-user' }),
    test: async () => TWO_SUM_RUN,
    submit: async () => MINIMAL_RUN,
    poll: async () => MINIMAL_RUN,
    runLocal: async () => TWO_SUM_RUN,
    format: async (p) => p.code,
    loginCaptureStatus: async () => ({ active: false, expiresAt: null }),
  };
}

function createFixtureVoice(): VoiceEditTransportPort {
  return {
    transcribe: async () => ({ transcript: FIXTURE_TRANSCRIPT, wallMs: 0 }),
    plan: async (r): Promise<VoiceEditResponse & { wallMs: number }> => ({
      transcript: r.transcript,
      alternatives: PLAN_ALTERNATIVES,
      wallMs: 0,
    }),
    sendEditStats: async () => {},
  };
}

function createFixtureAudio(): AudioRecordingPort {
  const listeners = new Set<(s: AudioLevel) => void>();
  let timer: ReturnType<typeof setInterval> | null = null;
  let index = 0;

  function emit(isRecording: boolean, durationMs: number, level: number): void {
    for (const listener of listeners) {
      listener({ isRecording, durationMs, level });
    }
  }

  return {
    requestPermission: async () => true,
    start: async () => {
      if (timer) return;
      index = 0;
      emit(true, 0, FIXTURE_LEVELS[0]);
      timer = setInterval(() => {
        index = (index + 1) % FIXTURE_LEVELS.length;
        emit(true, index * LEVEL_INTERVAL_MS, FIXTURE_LEVELS[index]);
      }, LEVEL_INTERVAL_MS);
    },
    stop: async () => {
      if (timer) {
        clearInterval(timer);
        timer = null;
      }
      emit(false, FIXTURE_RECORDING.durationMs, 0);
      return { ...FIXTURE_RECORDING };
    },
    subscribe: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}

export function createFixturePorts(): LeetCodePorts {
  return {
    gateway: createFixtureGateway(),
    voice: createFixtureVoice(),
    audio: createFixtureAudio(),
  };
}