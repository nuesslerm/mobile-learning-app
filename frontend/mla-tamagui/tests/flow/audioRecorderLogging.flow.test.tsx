import { act, FlowProviders, renderHook } from '@lib/flow-test-harness';
import { DependencyOverridesProvider } from '@lib/app-di';
import type { ReactNode } from 'react';
import { describe, expect, it } from 'vitest';

import {
  ExpoAudioRecorderProvider,
  useExpoAudioRecordingPort,
} from '../../features/leetcode/data/ExpoAudioRecorderAdapter';
import { forgetLoggedLines, loggedLines } from './loggedLines';

function Wrapper({ children }: { children: ReactNode }) {
  return (
    <FlowProviders>
      <DependencyOverridesProvider overrides={{}}>
        <ExpoAudioRecorderProvider>{children}</ExpoAudioRecorderProvider>
      </DependencyOverridesProvider>
    </FlowProviders>
  );
}

describe('Audio recorder adapter logging', () => {
  it('given the mic permission is asked, when the answer arrives, then the session logs the verdict', async () => {
    // Given a recorder adapter and an empty log
    const { result } = renderHook(() => useExpoAudioRecordingPort(), { wrapper: Wrapper });
    await forgetLoggedLines();

    // When the permission is requested
    await act(async () => {
      await result.current.requestPermission();
    });

    // Then one info line names the permission event with the granted verdict
    const lines = await loggedLines('audio.recorder.permission');
    expect(lines.map((line) => line.level)).toEqual(['info']);
    expect(lines[0]?.context).toMatchObject({ granted: true });
  });
});
