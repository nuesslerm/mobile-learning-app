import { DependencyOverridesProvider } from '@lib/app-di';
import { useMemo, type ReactNode } from 'react';

import { createFixturePorts } from '../data/fixturePorts';

export function LeetCodeFixtureOverrides({ children }: { children: ReactNode }) {
  const fixturePorts = useMemo(() => createFixturePorts(), []);
  if (process.env.EXPO_PUBLIC_LEETCODE_FIXTURES !== '1') {
    return children;
  }
  return (
    <DependencyOverridesProvider overrides={{ leetcode: fixturePorts }}>
      {children}
    </DependencyOverridesProvider>
  );
}