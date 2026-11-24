import { DependencyOverridesProvider } from '@lib/app-di';
import { useMemo, type ReactNode } from 'react';

import { createFixturePorts } from '../data/fixturePorts';
import { createDeviceLibrary } from '../data/library/deviceLibrary';
import { sampleLibrary } from '../data/library/libraryContent';

const replayLibrary = createDeviceLibrary('leetcode-library-replay-v1', sampleLibrary);

export function LeetCodeFixtureOverrides({ children }: { children: ReactNode }) {
  const fixturePorts = useMemo(() => ({ ...createFixturePorts(), libraryStorage: replayLibrary }), []);
  const replay = process.env.EXPO_PUBLIC_LEETCODE_LIBRARY_REPLAY === '1';
  const fixtures = process.env.EXPO_PUBLIC_LEETCODE_FIXTURES === '1';
  if (!fixtures && !replay) {
    return children;
  }
  return (
    <DependencyOverridesProvider overrides={{
      leetcode: fixtures ? fixturePorts : {
        libraryStorage: fixturePorts.libraryStorage,
        libraryContent: fixturePorts.libraryContent,
      },
    }}>
      {children}
    </DependencyOverridesProvider>
  );
}