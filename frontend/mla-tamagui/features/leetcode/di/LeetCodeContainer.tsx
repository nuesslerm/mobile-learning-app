import { useDependencyOverrides } from '@lib/app-di';
import { appConfig } from '@lib/app-config';
import { useMemo, type ReactNode } from 'react';

import type { AudioRecordingPort, LeetCodePortOverrides, LeetCodePorts } from '../domain/ports';
import { ExpoAudioRecorderProvider, useExpoAudioRecordingPort } from '../data/ExpoAudioRecorderAdapter';
import { createGatewayCodeRunner } from '../data/gatewayCodeRunner';
import { createLiveVoiceEditTransport } from '../data/voiceEditTransport';
import { createLiveLeetCodeGateway } from '../data/webBridgeClient';
import { LeetCodeContainerProvider } from '../state/ports';

const DEFAULT_BACKEND_BASE_URL = 'http://localhost:8789';

export function createDefaultContainer(
  overrides: LeetCodePortOverrides & { audio: AudioRecordingPort }
): LeetCodePorts {
  const gateway =
    overrides.gateway ??
    createLiveLeetCodeGateway(
      appConfig.leetcodeBackendBaseUrl ?? DEFAULT_BACKEND_BASE_URL
    );
  return {
    gateway,
    voice: overrides.voice ?? createLiveVoiceEditTransport(),
    audio: overrides.audio,
    runner: overrides.runner ?? createGatewayCodeRunner(gateway),
  };
}

function LeetCodePortsFromOverrides({ children }: { children: ReactNode }) {
  const o = useDependencyOverrides<LeetCodePortOverrides>('leetcode');
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
