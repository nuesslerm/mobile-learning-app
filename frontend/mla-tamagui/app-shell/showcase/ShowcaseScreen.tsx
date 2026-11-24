import {
  Button,
  Icon,
  ScreenHeader,
  Text,
  View,
} from '@lib/tamagui-design-system';
import { ChevronDown } from '@lib/tamagui-design-system/icons';
import { ScrollView } from '@lib/tamagui-design-system/primitives';
import {
  GlassView,
  isGlassEffectAPIAvailable,
  isLiquidGlassAvailable,
} from 'expo-glass-effect';
import { router, useLocalSearchParams } from 'expo-router';
import { useEffect, useState } from 'react';
import { Platform, StyleSheet } from 'react-native';

import { SHOWCASE_GROUPS } from './catalog';
import { SHOWCASE_ITEMS } from './items';
import { ItemSheet } from './ItemSheet';
import { OverlayHost } from './overlayHost';

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
  const initialItemId =
    SHOWCASE_ITEMS.find((item) => item.id === params.item)?.id ?? SHOWCASE_ITEMS[0].id;
  const [selectedId, setSelectedId] = useState(
    Platform.OS === 'web' ? SHOWCASE_ITEMS[0].id : initialItemId
  );
  // The static web export renders before the browser's query string is available.
  useEffect(() => setSelectedId(initialItemId), [initialItemId]);
  const [pickerOpen, setPickerOpen] = useState(false);

  const selected =
    SHOWCASE_ITEMS.find((item) => item.id === selectedId) ?? SHOWCASE_ITEMS[0];
  const group = SHOWCASE_GROUPS.find((entry) => entry.id === selected.group);
  const Selected = selected.Component;
  const glass =
    Platform.OS === 'ios' &&
    isLiquidGlassAvailable() &&
    isGlassEffectAPIAvailable();
  const pickerButton = (
    <Button
      variant={glass ? 'ghost' : 'outline'}
      rounded="$12"
      accessibilityLabel="Browse components"
      testID="ShowcaseItemButton"
      onPress={() => setPickerOpen(true)}
    >
      Browse components
      <Icon as={ChevronDown} size={16} />
    </Button>
  );

  return (
    <View flex={1} bg="$background">
      <ScreenHeader
        title="Design system"
        testID="ShowcaseNavigation"
        onBack={() =>
          router.canGoBack() ? router.back() : router.replace('/')
        }
      />
      <ScrollView flex={1} contentInsetAdjustmentBehavior="automatic">
        <View gap="$6" px="$5" py="$6">
          <View gap="$3" items="flex-start">
            {glass ? (
              <GlassView
                style={styles.picker}
                glassEffectStyle="regular"
                isInteractive
              >
                {pickerButton}
              </GlassView>
            ) : (
              pickerButton
            )}
            <View gap="$1" mt="$2">
              <Text variant="muted">{group?.title}</Text>
              <Text variant="h1" role="heading" testID="ShowcaseItemTitle">
                {selected.title}
              </Text>
            </View>
          </View>
          <Selected />
        </View>
      </ScrollView>

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
      <OverlayHost />
    </View>
  );
}

const styles = StyleSheet.create({
  picker: { borderRadius: 26 },
});
