import { DependencyOverridesProvider } from '@lib/app-di';
import { useGlobalSearchParams } from 'expo-router';
import { useMemo, type ReactNode } from 'react';

import { createFixturePorts } from '../data/fixturePorts';
import { createDeviceLibrary } from '../data/library/deviceLibrary';

const replayLibrary = createDeviceLibrary('leetcode-library-replay-v1');

export function LeetCodeFixtureOverrides({ children }: { children: ReactNode }) {
  const fixturePorts = useMemo(() => ({ ...createFixturePorts(), libraryStorage: replayLibrary }), []);
  const replay = process.env.EXPO_PUBLIC_LEETCODE_LIBRARY_REPLAY === '1';
  const fixtures = process.env.EXPO_PUBLIC_LEETCODE_FIXTURES === '1';
  const { voiceReplay } = useGlobalSearchParams<{ voiceReplay?: string }>();
  const nativeVoice = process.env.EXPO_PUBLIC_DEVICE_LAB === '1' && voiceReplay === '1';
  if (!fixtures && !replay && !nativeVoice) {
    return children;
  }
  return (
    <DependencyOverridesProvider overrides={{
      leetcode: fixtures ? fixturePorts : {
        ...(replay && { libraryStorage: fixturePorts.libraryStorage, libraryContent: fixturePorts.libraryContent }),
        ...(nativeVoice && { audio: fixturePorts.audio, voice: fixturePorts.voice, flags: { aiEdit: true } }),
      },
    }}>
      {children}
    </DependencyOverridesProvider>
  );
}