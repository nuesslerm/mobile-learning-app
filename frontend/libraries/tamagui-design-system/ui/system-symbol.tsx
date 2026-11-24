import type { ImageProps } from '@expo/ui/swift-ui';
import type { ReactElement } from 'react';

export type SystemSymbolName = NonNullable<ImageProps['systemName']>;

export function renderSystemSymbol(_name: SystemSymbolName, _size: number, _color: string, _testID?: string): ReactElement | null {
  return null;
}
