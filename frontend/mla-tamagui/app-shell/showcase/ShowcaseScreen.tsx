import { Button, Text, View } from '@lib/tamagui-design-system';
import { ChevronDown } from '@lib/tamagui-design-system/icons';
import { ScrollView } from '@lib/tamagui-design-system/primitives';
import { useLocalSearchParams } from 'expo-router';
import { useState } from 'react';

import { SHOWCASE_GROUPS } from './catalog';
import { SHOWCASE_ITEMS } from './items';
import { ItemSheet } from './ItemSheet';

/**
 * Exercises the design system's real components rather than raw Tamagui, so
 * what renders here is the thing the leetcode port will be built from. One
 * item at a time, picked from a bottom sheet (`?item=<id>` opens one directly),
 * each shown beside its Expo UI counterpart where Expo UI has one.
 *
 * testIDs are hand-written and semantic. This app is exempt from the repo's
 * testID lint machinery, but the browser check needs stable selectors.
 */
export function ShowcaseScreen() {
  const params = useLocalSearchParams<{ item?: string }>();
  const [selectedId, setSelectedId] = useState(
    SHOWCASE_ITEMS.some((item) => item.id === params.item)
      ? (params.item as string)
      : SHOWCASE_ITEMS[0].id
  );
  const [pickerOpen, setPickerOpen] = useState(false);

  const selected =
    SHOWCASE_ITEMS.find((item) => item.id === selectedId) ?? SHOWCASE_ITEMS[0];
  const group = SHOWCASE_GROUPS.find((entry) => entry.id === selected.group);
  const Selected = selected.Component;

  return (
    <ScrollView bg="$background">
      <View gap="$4" p="$4" pt="$8">
        <Text variant="h1" testID="ShowcaseHeading">
          Design system
        </Text>
        <Text variant="muted" testID="ShowcaseSubtitle">
          Tamagui components, same names as app #1, beside their Expo UI
          counterparts
        </Text>

        <Button
          variant="outline"
          icon={ChevronDown}
          testID="ShowcaseItemButton"
          onPress={() => setPickerOpen(true)}
        >
          {`${group?.title ?? ''}: ${selected.title}`}
        </Button>

        <Text variant="h2" testID="ShowcaseItemTitle">
          {selected.title}
        </Text>
        <Selected />
      </View>

      <ItemSheet
        open={pickerOpen}
        onOpenChange={setPickerOpen}
        items={SHOWCASE_ITEMS}
        selectedId={selected.id}
        onSelect={(id) => {
          setSelectedId(id);
          setPickerOpen(false);
        }}
      />
    </ScrollView>
  );
}
