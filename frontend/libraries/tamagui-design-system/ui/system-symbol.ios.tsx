import { Host, Image, type ImageProps } from '@expo/ui/swift-ui';

export function renderSystemSymbol(name: NonNullable<ImageProps['systemName']>, size: number, color: string, testID?: string) {
  return (
    <Host matchContents pointerEvents="none" testID={testID}>
      <Image systemName={name} size={size} color={color} />
    </Host>
  );
}
