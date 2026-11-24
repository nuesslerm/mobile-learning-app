import { List, type ListItemProps } from '@expo/ui';
import type { ReactNode } from 'react';

export const showcaseNativeRowModifiers: ListItemProps['modifiers'] = undefined;

export function ShowcaseNativeList({ children }: { children: ReactNode }) {
  return <List testID="ShowcaseExpoList">{children}</List>;
}
