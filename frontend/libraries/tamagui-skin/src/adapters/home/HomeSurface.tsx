import { YStack } from '@lib/tamagui-design-system/primitives';
import type { ReactNode } from 'react';

export function HomeSurface({ children }: { children: ReactNode }) {
  return (
    <YStack flex={1} bg="$background">
      {children}
    </YStack>
  );
}
