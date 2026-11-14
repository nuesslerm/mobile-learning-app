import { useDependencyOverrides } from '@lib/app-di';
import { appConfig } from '@lib/app-config';
import { useMemo, type ReactNode } from 'react';

import type { AudioRecordingPort, LeetCodePorts } from '../domain/ports';
import { ExpoAudioRecorderProvider, useExpoAudioRecordingPort } from '../data/ExpoAudioRecorderAdapter';
import { createLiveVoiceEditTransport } from '../data/voiceEditTransport';
import { createLiveLeetCodeGateway } from '../data/webBridgeClient';
import { LeetCodeContainerProvider } from '../state/ports';

const DEFAULT_BACKEND_BASE_URL = 'http://localhost:8789';

export function createDefaultContainer(
  overrides: Partial<LeetCodePorts> & { audio: AudioRecordingPort }
): LeetCodePorts {
  return {
    gateway:
      overrides.gateway ??
      createLiveLeetCodeGateway(
        appConfig.leetcodeBackendBaseUrl ?? DEFAULT_BACKEND_BASE_URL
      ),
    voice: overrides.voice ?? createLiveVoiceEditTransport(),
    audio: overrides.audio,
  };
}

function LeetCodePortsFromOverrides({ children }: { children: ReactNode }) {
  const o = useDependencyOverrides<LeetCodePorts>('leetcode');
  const audio = useExpoAudioRecordingPort();
  const ports = useMemo(
    () => createDefaultContainer({ ...o, audio: o.audio ?? audio }),
    [o, audio]
  );
  return <LeetCodeContainerProvider value={ports}>{children}</LeetCodeContainerProvider>;
}

export function LeetCodeLiveContainer({ children }: { children: ReactNode }) {
  return (
    <ExpoAudioRecorderProvider>
      <LeetCodePortsFromOverrides>{children}</LeetCodePortsFromOverrides>
    </ExpoAudioRecorderProvider>
  );
}
