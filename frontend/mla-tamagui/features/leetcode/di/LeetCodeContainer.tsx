import { useDependencyOverrides } from '@lib/app-di';
import { appConfig } from '@lib/app-config';
import { useMemo, type ReactNode } from 'react';

import type { AudioRecordingPort, LeetCodePortOverrides, LeetCodePorts } from '../domain/ports';
import { ExpoAudioRecorderProvider, useExpoAudioRecordingPort } from '../data/ExpoAudioRecorderAdapter';
import { createGatewayCodeRunner } from '../data/gatewayCodeRunner';
import { createSelectingCodeRunner } from '../data/selectingCodeRunner';
import { useOptionalPyodideCodeRunner } from '../data/pyodide/PyodideRunnerProvider';
import { createUnavailableCodeRunner } from '../data/unavailableCodeRunner';
import { createLiveVoiceEditTransport } from '../data/voiceEditTransport';
import { createLiveLeetCodeGateway } from '../data/webBridgeClient';
import { LeetCodeContainerProvider } from '../state/ports';
import { useRunnerSettingStore, whenRunnerSettingHydrated } from '../state/runnerSettingStore';

const DEFAULT_BACKEND_BASE_URL = 'http://localhost:8789';

export function createDefaultContainer(
  overrides: LeetCodePortOverrides & { audio: AudioRecordingPort }
): LeetCodePorts {
  const gateway =
    overrides.gateway ??
    createLiveLeetCodeGateway(
      appConfig.leetcodeBackendBaseUrl ?? DEFAULT_BACKEND_BASE_URL
    );
  const deviceRunner = overrides.deviceRunner ?? createUnavailableCodeRunner();
  return {
    gateway,
    voice: overrides.voice ?? createLiveVoiceEditTransport(),
    audio: overrides.audio,
    deviceRunner,
    runner:
      overrides.runner ??
      createSelectingCodeRunner({
        choose: async () => {
          await whenRunnerSettingHydrated();
          return useRunnerSettingStore.getState().runner;
        },
        current: () => useRunnerSettingStore.getState().runner,
        runners: { backend: createGatewayCodeRunner(gateway), device: deviceRunner },
      }),
  };
}

function LeetCodePortsFromOverrides({ children }: { children: ReactNode }) {
  const o = useDependencyOverrides<LeetCodePortOverrides>('leetcode');
  const audio = useExpoAudioRecordingPort();
  // the on-device runner the root provides, when the tree has one
  const device = useOptionalPyodideCodeRunner();
  const ports = useMemo(
    () => createDefaultContainer({ ...o, audio: o.audio ?? audio, deviceRunner: o.deviceRunner ?? device }),
    [o, audio, device]
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
