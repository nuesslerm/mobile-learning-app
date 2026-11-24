import {
  Icon,
  IconButton,
  InsetGroup,
  ListItem,
  Text,
  View,
} from '@lib/tamagui-design-system';
import { Check, X } from '@lib/tamagui-design-system/icons';
import { XStack } from '@lib/tamagui-design-system/primitives';
import { Sheet } from '@lib/tamagui-design-system/ui/sheet';
import { StyleSheet } from 'react-native';

import { SHOWCASE_GROUPS, type ShowcaseItem } from './catalog';

type Props = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  items: ShowcaseItem[];
  selectedId: string;
  onSelect: (id: string) => void;
};

/** The item picker: every showcase item, listed under its item type, in a bottom sheet. */
export function ItemSheet({
  open,
  onOpenChange,
  items,
  selectedId,
  onSelect,
}: Props) {
  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      floating
      snapPoints={[62]}
      testID="ShowcaseItemSheet"
    >
      <XStack items="center" testID="ShowcaseItemSheetHeader">
        <View width={44} />
        <Text variant="large" role="heading" flex={1} style={styles.title}>
          Choose a component
        </Text>
        <IconButton
          icon={X}
          accessibilityLabel="Close"
          onPress={() => onOpenChange(false)}
          testID="ShowcaseItemSheetHeaderClose"
        />
      </XStack>
      <Sheet.ScrollView style={styles.options}>
        <View gap="$3" px="$3" py="$3">
          {SHOWCASE_GROUPS.map((group) => (
            <View key={group.id} gap="$2">
              <Text variant="muted" testID={`ShowcaseItemGroup-${group.id}`}>
                {group.title}
              </Text>
              <InsetGroup
                inset={false}
                surface="transparent"
                testID={`ShowcaseItemList-${group.id}`}
              >
                {items
                  .filter((item) => item.group === group.id)
                  .map((item) => (
                    <ListItem
                      key={item.id}
                      variant="grouped"
                      testID={`ShowcaseItem-${item.id}`}
                      onPress={() => onSelect(item.id)}
                    >
                      <XStack items="center" justify="space-between">
                        <Text numberOfLines={1} style={styles.rowLabel}>
                          {item.title}
                        </Text>
                        {item.id === selectedId ? (
                          <Icon as={Check} size={16} color="$color11" />
                        ) : null}
                      </XStack>
                    </ListItem>
                  ))}
              </InsetGroup>
            </View>
          ))}
        </View>
      </Sheet.ScrollView>
    </Sheet>
  );
}

const styles = StyleSheet.create({
  title: { textAlign: 'center' },
  rowLabel: { flexShrink: 1 },
  options: {
    backgroundColor: 'rgba(127, 127, 127, 0.10)',
    borderRadius: 20,
    marginBottom: 12,
    overflow: 'hidden',
  },
});
