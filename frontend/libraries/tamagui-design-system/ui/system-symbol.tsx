import type { ImageProps } from '@expo/ui/swift-ui';
import type { ReactElement } from 'react';

export type SystemSymbolName = NonNullable<ImageProps['systemName']>;
export type SystemSymbol = SystemSymbolName | {
  name: SystemSymbolName;
  weight: 'medium' | 'semibold';
};

export function renderSystemSymbol(_symbol: SystemSymbol, _size: number, _color: string, _testID?: string): ReactElement | null {
  return null;
}
