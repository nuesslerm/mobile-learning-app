import {
  BUILTIN_KEY_IDS,
  CUSTOM_KEY_LABEL_MAX,
  toolbarItemKey,
  type ToolbarItem,
} from '../domain';
import { Logger } from '@lib/logging';
import {
  Button,
  Icon,
  IconButton,
  Separator,
  Text,
  TextField,
  View,
} from '@lib/tamagui-design-system';
import { Stack } from 'expo-router';
import { CircleMinus, Plus } from '@lib/tamagui-design-system/icons';
import { useCallback, useRef, useState, type ReactNode } from 'react';
// gesture-handler's ScrollView, not react-native's: its ref resolves to a real
// RNScrollView with a working imperative `scrollTo`, which the auto-scroll in
// ReorderableList depends on.
import {
  GestureHandlerRootView,
  ScrollView,
} from 'react-native-gesture-handler';
import { useSharedValue } from 'react-native-reanimated';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';

import { BUILTIN_KEYS } from '../code-editor/core/toolbarKeys';
import { useToolbarConfigStore } from '../state/toolbarConfigStore';

import { ReorderableList } from './ReorderableList';

const FILL = { flex: 1 };
const SCROLL_CONTENT = {
  paddingHorizontal: 16,
  paddingTop: 8,
  paddingBottom: 40,
};
const ROW_HEIGHT = 64;

/** Icon or monospace glyph preview of a key, in a fixed-size box. */
function KeyPreview({ icon, label }: { icon?: typeof Plus; label?: string }) {
  return (
    <YStack
      width={36}
      height={36}
      items="center"
      justify="center"
      rounded="$3"
      bg="$color3"
      testID="ToolbarKeyPreview"
    >
      {icon ? (
        <Icon as={icon} size={20} />
      ) : (
        <Text variant="code">{label}</Text>
      )}
    </YStack>
  );
}

function describe(item: ToolbarItem): {
  name: string;
  icon?: typeof Plus;
  label?: string;
} {
  if (item.kind === 'builtin') {
    const def = BUILTIN_KEYS[item.id];
    return { name: def.name, icon: def.icon, label: def.label };
  }
  return { name: `Inserts “${item.insert}”`, label: item.label };
}

function ConfigRow({ item, handle }: { item: ToolbarItem; handle: ReactNode }) {
  const removeKey = useToolbarConfigStore((s) => s.removeKey);
  const info = describe(item);

  return (
    <XStack
      height={56}
      items="center"
      gap="$2"
      px="$3"
      rounded="$4"
      bg="$color2"
      testID="ToolbarConfigRow"
    >
      <IconButton
        icon={CircleMinus}
        variant="ghost"
        testID="ToolbarRemove"
        aria-label={`Remove ${info.name}`}
        pressLog="off"
        color="$red10"
        onPress={() => {
          Logger.info('leetcode.toolbar.remove', {
            itemKey: toolbarItemKey(item),
            kind: item.kind,
          });
          removeKey(toolbarItemKey(item));
        }}
      />
      <KeyPreview icon={info.icon} label={info.label} />
      <Text flex={1} numberOfLines={1} testID="ToolbarConfigRowName">
        {info.name}
      </Text>
      {handle}
    </XStack>
  );
}

function AddCommandSection() {
  const items = useToolbarConfigStore((s) => s.items);
  const addBuiltin = useToolbarConfigStore((s) => s.addBuiltin);
  const available = BUILTIN_KEY_IDS.filter(
    (id) => !items.some((it) => it.kind === 'builtin' && it.id === id)
  );

  if (available.length === 0) return null;

  return (
    <YStack mt="$4" testID="ToolbarAddSection">
      <Text variant="muted" mb="$2" px="$1">
        Add command
      </Text>
      <YStack bg="$color2" rounded="$4" overflow="hidden">
        {available.map((id, index) => {
          const def = BUILTIN_KEYS[id];
          return (
            <YStack key={id}>
              {index > 0 ? <Separator /> : null}
              <XStack
                testID="ToolbarAdd"
                role="button"
                aria-label={`Add ${def.name}`}
                items="center"
                gap="$3"
                px="$3"
                py="$2.5"
                pressStyle={{ bg: '$color4' }}
                cursor="pointer"
                onPress={() => {
                  Logger.info('leetcode.toolbar.add_builtin', { id });
                  addBuiltin(id);
                }}
              >
                <Icon as={Plus} size={20} color="$blue10" />
                <KeyPreview icon={def.icon} label={def.label} />
                <Text flex={1}>{def.name}</Text>
              </XStack>
            </YStack>
          );
        })}
      </YStack>
    </YStack>
  );
}

function CreateCustomSection() {
  const addCustom = useToolbarConfigStore((s) => s.addCustom);
  const [label, setLabel] = useState('');
  const [insert, setInsert] = useState('');

  const canAdd = label.trim().length > 0 && insert.length > 0;

  return (
    <YStack mt="$4" testID="ToolbarCustomSection">
      <Text variant="muted" mb="$2" px="$1">
        Create custom key
      </Text>
      <YStack bg="$color2" rounded="$4" gap="$3" p="$3">
        <TextField
          label="Key label (shown on the bar)"
          value={label}
          onChangeText={setLabel}
          placeholder="e.g. →"
          maxLength={CUSTOM_KEY_LABEL_MAX}
          autoCapitalize="none"
          autoCorrect={false}
          testID="ToolbarLabelInput"
        />
        <TextField
          label="Text to insert"
          value={insert}
          onChangeText={setInsert}
          placeholder="e.g. self."
          autoCapitalize="none"
          autoCorrect={false}
          testID="ToolbarInsertInput"
        />
        <Button
          variant="secondary"
          testID="ToolbarAddCustom"
          disabled={!canAdd}
          pressLog="off"
          onPress={() => {
            if (!canAdd) return;
            addCustom({ label: label.trim(), insert });
            setLabel('');
            setInsert('');
          }}
        >
          Add custom key
        </Button>
      </YStack>
    </YStack>
  );
}

export function ToolbarConfigScreen(): React.ReactElement {
  const items = useToolbarConfigStore((s) => s.items);
  const setItems = useToolbarConfigStore((s) => s.setItems);
  const resetToDefault = useToolbarConfigStore((s) => s.resetToDefault);

  const scrollViewRef = useRef<ScrollView>(null);
  const scrollOffset = useSharedValue(0);

  const scrollToY = useCallback((y: number) => {
    scrollViewRef.current?.scrollTo({ y, animated: false });
  }, []);

  const handleScroll = useCallback(
    (y: number) => {
      scrollOffset.value = y;
    },
    [scrollOffset]
  );

  return (
    <GestureHandlerRootView style={FILL}>
      <Stack.Screen options={{ title: 'Toolbar' }} />
      <View flex={1} bg="$background" testID="ToolbarConfigScreen">
        <ScrollView
          ref={scrollViewRef}
          testID="ToolbarScroll"
          onScroll={(e) => {
            handleScroll(e.nativeEvent.contentOffset.y);
          }}
          scrollEventThrottle={16}
          contentContainerStyle={SCROLL_CONTENT}
          keyboardShouldPersistTaps="handled"
        >
          <Text variant="muted" mb="$2" px="$1">
            Long-press the handle to drag · tap − to remove
          </Text>

          <ReorderableList
            data={items}
            keyExtractor={toolbarItemKey}
            rowHeight={ROW_HEIGHT}
            onReorder={setItems}
            scrollOffset={scrollOffset}
            scrollToY={scrollToY}
            renderItem={(item, handle) => (
              <ConfigRow item={item} handle={handle} />
            )}
          />

          <AddCommandSection />
          <CreateCustomSection />

          <Button
            variant="ghost"
            testID="ToolbarReset"
            mt="$4"
            pressLog="off"
            onPress={() => {
              Logger.info('leetcode.toolbar.reset', {});
              resetToDefault();
            }}
          >
            <Text color="$red10">Reset to default</Text>
          </Button>
        </ScrollView>
      </View>
    </GestureHandlerRootView>
  );
}
