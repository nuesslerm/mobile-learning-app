import { useDependencyOverrides } from '@lib/app-di';
import { appConfig } from '@lib/app-config';
import { createContext, useContext, useMemo, type ReactNode } from 'react';

import type { AudioRecordingPort, LeetCodePorts } from '../contracts/ports';
import { useExpoAudioRecordingPort } from '../data/ExpoAudioRecorderAdapter';
import { createLiveVoiceEditTransport } from '../data/voiceEditTransport';
import { createLiveLeetCodeGateway } from '../data/webBridgeClient';

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

const LeetCodeContainerContext = createContext<LeetCodePorts | null>(null);

export function LeetCodeContainerProvider({
  value,
  children,
}: {
  value: LeetCodePorts;
  children: ReactNode;
}) {
  return (
    <LeetCodeContainerContext.Provider value={value}>
      {children}
    </LeetCodeContainerContext.Provider>
  );
}

export function useLeetCodePorts(): LeetCodePorts {
  const ports = useContext(LeetCodeContainerContext);
  if (!ports) {
    throw new Error('LeetCodeContainerProvider missing');
  }
  return ports;
}

// Must render inside ExpoAudioRecorderProvider: the audio port comes from its context.
export function LeetCodeLiveContainer({ children }: { children: ReactNode }) {
  const o = useDependencyOverrides<LeetCodePorts>('leetcode');
  const audio = useExpoAudioRecordingPort();
  const ports = useMemo(
    () => createDefaultContainer({ ...o, audio: o.audio ?? audio }),
    [o, audio]
  );
  return <LeetCodeContainerProvider value={ports}>{children}</LeetCodeContainerProvider>;
}
