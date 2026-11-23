import {
  Icon,
  InsetGroup,
  ListItem,
  Text,
  View,
} from '@lib/tamagui-design-system';
import { Check } from '@lib/tamagui-design-system/icons';
import { XStack } from '@lib/tamagui-design-system/primitives';
import { Sheet, SheetHeader } from '@lib/tamagui-design-system/ui/sheet';

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
      snapPoints={[80]}
      testID="ShowcaseItemSheet"
    >
      <SheetHeader
        onClose={() => onOpenChange(false)}
        testID="ShowcaseItemSheetHeader"
      >
        <Text variant="large">Items</Text>
      </SheetHeader>
      <Sheet.ScrollView>
        <View gap="$3" p="$4">
          {SHOWCASE_GROUPS.map((group) => (
            <View key={group.id} gap="$2">
              <Text variant="small" testID={`ShowcaseItemGroup-${group.id}`}>
                {group.title.toUpperCase()}
              </Text>
              <InsetGroup testID={`ShowcaseItemList-${group.id}`}>
                {items
                  .filter((item) => item.group === group.id)
                  .map((item) => (
                    <ListItem
                      key={item.id}
                      variant="grouped"
                      testID={`ShowcaseItem-${item.id}`}
                      onPress={() => onSelect(item.id)}
                    >
                      <XStack items="center" justify="space-between" flex={1}>
                        <Text>{item.title}</Text>
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
