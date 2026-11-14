import {
  act,
  createFakeAudioRecorder,
  createFakeVoiceEditTransport,
  FlowProviders,
  renderHook,
} from '@lib/flow-test-harness';
import { DependencyOverridesProvider } from '@lib/app-di';
import type { ReactNode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { ExpoAudioRecorderProvider } from '../../features/leetcode/data/ExpoAudioRecorderAdapter';
import { LeetCodeLiveContainer } from '../../features/leetcode/di/LeetCodeContainer';
import { useLiveTranscript } from '../../features/leetcode/plan/state/useLiveTranscript';

type Transcript = { transcript: string; wallMs: number };
const sentChunkIndexes: (number | undefined)[] = [];
const deferreds: ((value: Transcript) => void)[] = [];
const sendEditStats = vi.fn(async (_row: unknown) => undefined);
const sendVoiceTranscribe = vi.fn((req: { chunkIndex?: number }) => {
  sentChunkIndexes.push(req.chunkIndex);
  return new Promise<Transcript>((resolve) => {
    deferreds.push(resolve);
  });
});

const CHUNK_MS = 100;

describe('useLiveTranscript', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    sentChunkIndexes.length = 0;
    deferreds.length = 0;
    sendVoiceTranscribe.mockClear();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('given three chunks whose second response arrives last, when started then stopped, then text is in chunk order', async () => {
    // Given the leetcode composition root booted with scripted recorder and transport
    const audio = createFakeAudioRecorder();
    const voice = {
      ...createFakeVoiceEditTransport(),
      transcribe: (req: { chunkIndex?: number }) => sendVoiceTranscribe(req),
      sendEditStats: (row: unknown) => sendEditStats(row),
    };
    const overrides = { leetcode: { audio, voice } };
    // app.renderHook's fixed wrapper cannot mount the leetcode composition
    // root, which useVoiceRecording needs via useLeetCodePorts; boot it here,
    // stacked the same way the leetcode route layout stacks it.
    function Wrapper({ children }: { children: ReactNode }) {
      return (
        <FlowProviders>
          <DependencyOverridesProvider overrides={overrides}>
            <ExpoAudioRecorderProvider>
              <LeetCodeLiveContainer>{children}</LeetCodeLiveContainer>
            </ExpoAudioRecorderProvider>
          </DependencyOverridesProvider>
        </FlowProviders>
      );
    }
    const { result } = renderHook(
      () => useLiveTranscript({ editId: 'edit-1', chunkMs: CHUNK_MS }),
      { wrapper: Wrapper }
    );

    // When the session starts and two interval chunks are cut
    await act(async () => {
      await result.current.start();
    });
    expect(result.current.listening).toBe(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(CHUNK_MS);
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(CHUNK_MS);
    });
    expect(sentChunkIndexes).toEqual([0, 1]);

    // And stop cuts the final clip while responses arrive out of order
    await act(async () => {
      const stopPromise = result.current.stop();
      await vi.advanceTimersByTimeAsync(0);
      expect(deferreds).toHaveLength(3);
      deferreds[0]?.({ transcript: 'alpha', wallMs: 10 });
      deferreds[2]?.({ transcript: 'gamma', wallMs: 30 });
      deferreds[1]?.({ transcript: 'beta', wallMs: 20 });
      await stopPromise;
    });

    // Then all three chunks were sent in order and appended in chunk order
    expect(sentChunkIndexes).toEqual([0, 1, 2]);
    expect(result.current.text).toBe('alpha beta gamma');
    expect(result.current.chunkMs).toEqual([10, 20, 30]);
    expect(result.current.listening).toBe(false);
  });
});