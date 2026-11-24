import { Button, Icon, Text, View } from '@lib/tamagui-design-system';
import { Menu } from '@lib/tamagui-design-system/icons';
import { useNavigation } from 'expo-router';

export function DrawerItem() {
  const navigation = useNavigation();

  return (
    <View items="flex-start" gap="$4">
      <Text variant="muted">
        The page moves aside to reveal app navigation. Tap the page to close it.
      </Text>
      <Button
        variant="outline"
        rounded="$12"
        testID="ShowcaseOpenDrawer"
        onPress={() => navigation.getParent()?.dispatch({ type: 'OPEN_DRAWER' })}
      >
        <Icon as={Menu} size={16} />
        Open drawer
      </Button>
    </View>
  );
}
