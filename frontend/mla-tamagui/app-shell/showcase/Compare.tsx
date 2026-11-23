import { Separator, Text, View } from '@lib/tamagui-design-system';
import { Column, Host } from '@expo/ui';
import type { ReactNode } from 'react';

type Props = {
  tamagui: ReactNode;
  /** The Expo UI counterpart; leave out when Expo UI has none. */
  expoUi?: ReactNode;
  testID: string;
};

/** One item twice: the design system's Tamagui component, then the Expo UI one drawn by the platform's own toolkit. */
export function Compare({ tamagui, expoUi, testID }: Props) {
  return (
    <View gap="$3" testID={testID}>
      <Text variant="small" testID={`${testID}-TamaguiLabel`}>
        TAMAGUI (DESIGN SYSTEM)
      </Text>
      <View gap="$2" testID={`${testID}-Tamagui`}>
        {tamagui}
      </View>
      <Separator />
      <Text variant="small" testID={`${testID}-ExpoUiLabel`}>
        EXPO UI
      </Text>
      {expoUi ? (
        <View gap="$2" testID={`${testID}-ExpoUi`}>
          {expoUi}
        </View>
      ) : (
        <Text variant="muted" testID={`${testID}-NoExpoUi`}>
          No Expo UI equivalent.
        </Text>
      )}
    </View>
  );
}

/** Expo UI draws inside a Host that sizes to its content; a Column keeps several controls apart and the Host spans the row. */
export function ExpoHost({ children }: { children: ReactNode }) {
  return (
    <Host matchContents={{ vertical: true }} style={{ width: '100%' }}>
      <Column spacing={12} alignment="start">
        {children}
      </Column>
    </Host>
  );
}
