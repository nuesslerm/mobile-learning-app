import { Separator, Text, View } from '@lib/tamagui-design-system';
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
