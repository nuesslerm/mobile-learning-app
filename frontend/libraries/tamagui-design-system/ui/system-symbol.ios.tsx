import { Host, Image } from '@expo/ui/swift-ui';
import { font } from '@expo/ui/swift-ui/modifiers';

import type { SystemSymbol } from './system-symbol';

export function renderSystemSymbol(symbol: SystemSymbol, size: number, color: string, testID?: string) {
  const { name, weight } = typeof symbol === 'string' ? { name: symbol, weight: undefined } : symbol;
  return (
    <Host matchContents pointerEvents="none" testID={testID}>
      <Image systemName={name} size={size} color={color} modifiers={weight ? [font({ size, weight })] : undefined} />
    </Host>
  );
}
