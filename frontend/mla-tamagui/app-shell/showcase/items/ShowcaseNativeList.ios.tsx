import { List } from '@expo/ui/swift-ui';
import {
  listRowBackground,
  listStyle,
  scrollContentBackground,
} from '@expo/ui/swift-ui/modifiers';
import type { ReactNode } from 'react';

export const showcaseNativeRowModifiers = [listRowBackground('transparent')];

export function ShowcaseNativeList({ children }: { children: ReactNode }) {
  return (
    <List
      modifiers={[listStyle('plain'), scrollContentBackground('hidden')]}
      testID="ShowcaseExpoList"
    >
      {children}
    </List>
  );
}
