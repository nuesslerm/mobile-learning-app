import { Logger } from '@lib/logging';
import { Button, Input, PageSheet, Switch, Text } from '@lib/tamagui-design-system';
import { Menu } from '@lib/tamagui-design-system/icons';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';
import { Sheet, SheetHeader } from '@lib/tamagui-design-system/ui/sheet';
import { useCallback, type ReactNode } from 'react';
import { Platform, ScrollView, StyleSheet, View, useColorScheme } from 'react-native';

import {
  groupLabel,
  isDefaultGroups,
  moveBetween,
  type SymbolGroup,
} from '../../domain/editor-settings/symbolGroups';
import {
  RAIL_STYLES,
  useSymbolBarStore,
} from '../../state/editor/symbolBarStore';
import { ReorderableList } from '../ReorderableList';

import { RAIL_STYLE_LABELS } from './railStyleMenu';

const nativePage = Platform.OS === 'ios';
const ROW_HEIGHT = nativePage ? 55.75 : 56;
const CHARS_STYLE = { fontFamily: nativePage ? 'Menlo' : 'Menlo, Courier, monospace', textAlign: 'right' } as const;
const LIFTED_ROW = {
  dark: { backgroundColor: '#19181b', left: -11, right: -1 },
  light: { backgroundColor: '#fff', left: -11, right: -1 },
} as const;

const pageColors = {
  dark: { page: '#0d0d0f', group: '#1b1a1d', divider: '#272629', pill: '#29282b', pillBorder: '#444247', label: '#fff', caption: '#8d8d93', handle: '#555459' },
  light: { page: '#f2f2f7', group: '#ffffff', divider: '#dedee3', pill: '#e5e5ea', pillBorder: '#d1d1d6', label: '#17171a', caption: '#696970', handle: '#8e8e93' },
} as const;
type PageColors = (typeof pageColors)[keyof typeof pageColors];

type Props = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onDismiss?: () => void;
  showRailStyle?: boolean;
  testID?: string;
};

function GroupRow({
  group,
  index,
  last,
  colors,
  handle,
}: {
  group: SymbolGroup;
  index: number;
  last: boolean;
  colors?: PageColors;
  handle: ReactNode;
}) {
  const setGroup = useSymbolBarStore((s) => s.setGroup);
  return (
    <XStack
      height={ROW_HEIGHT}
      items="center"
      gap={colors ? 0 : '$3'}
      pl={colors ? 21 : '$4'}
      pr={colors ? 12 : 0}
      testID={`SymbolBarRow-${group.id}`}
    >
      <Text flex={1} color={colors?.label ?? "$color"} fontSize={colors ? 18 : undefined} testID={`SymbolBarRowLabel-${group.id}`}>
        {groupLabel(index)}
      </Text>
      <Input
        testID={`SymbolBarChars-${group.id}`}
        aria-label={`${groupLabel(index)} characters`}
        value={group.chars}
        width={colors ? 84 : 104}
        autoCapitalize="none"
        autoCorrect={false}
        borderWidth={0}
        bg="transparent"
        {...(colors ? { px: 0 } : {})}
        style={colors ? { ...CHARS_STYLE, color: colors.label, fontSize: 18 } : CHARS_STYLE}
        onChangeText={(text) => setGroup(group.id, text)}
      />
      {handle}
      {colors && !last ? <View pointerEvents="none" style={[styles.divider, { backgroundColor: colors.divider }]} /> : null}
    </XStack>
  );
}

/**
 * The recording's Symbol Bar sheet: the switch, the eight groups (each edited in place and reordered by its handle) and Reset and Done. It reads and writes only the symbol bar store; the caller owns whether it is open.
 * The Classic | Symbols control switches back to the classic character-key rail; Symbols is the default.
 */
export function SymbolBarSheet({ open, onOpenChange, onDismiss, showRailStyle = true, testID = 'SymbolBarSheet' }: Props) {
  const scheme = useColorScheme();
  const colors = nativePage ? pageColors[scheme === 'dark' ? 'dark' : 'light'] : undefined;
  const shown = useSymbolBarStore((s) => s.shown);
  const groups = useSymbolBarStore((s) => s.groups);
  const railStyle = useSymbolBarStore((s) => s.railStyle);
  const setShown = useSymbolBarStore((s) => s.setShown);
  const setRailStyle = useSymbolBarStore((s) => s.setRailStyle);
  const move = useSymbolBarStore((s) => s.move);
  const reset = useSymbolBarStore((s) => s.reset);

  const onReorder = useCallback(
    (next: SymbolGroup[]) => {
      const moved = moveBetween(groups, next);
      if (moved) move(moved.from, moved.to);
    },
    [groups, move]
  );
  const keyOf = useCallback((group: SymbolGroup) => group.id, []);
  const indexOf = (group: SymbolGroup) => groups.findIndex((candidate) => candidate.id === group.id);

  const BodyScrollView = nativePage ? ScrollView : Sheet.ScrollView;
  const headerPill = colors && {
    height: 44,
    minW: 76,
    fontSize: 17,
    fontWeight: '500' as const,
    color: colors.label,
    style: { backgroundColor: colors.pill, borderColor: colors.pillBorder, borderWidth: StyleSheet.hairlineWidth },
  };
  const content = (
    <>
      <SheetHeader
        testID={`${testID}Header`}
        style={colors && styles.header}
        leading={
          <Button
            testID="SymbolBarReset"
            variant={nativePage ? "secondary" : "outline"}
            size="$3"
            rounded="$10"
            {...headerPill}
            disabled={!nativePage && isDefaultGroups(groups)}
            pressLog="off"
            onPress={() => {
              Logger.info('leetcode.symbol_bar.reset_press', {});
              reset();
            }}
          >
            Reset
          </Button>
        }
        trailing={
          <Button
            testID="SymbolBarDone"
            variant={nativePage ? "secondary" : "default"}
            size="$3"
            rounded="$10"
            {...headerPill}
            pressLog="off"
            onPress={() => {
              Logger.info('leetcode.symbol_bar.done_press', {});
              onOpenChange(false);
            }}
          >
            Done
          </Button>
        }
      >
        <Text variant="h4" text="center" fontSize={colors ? 17 : undefined} color={colors?.label ?? "$color"} testID="SymbolBarTitle">
          Symbol Bar
        </Text>
      </SheetHeader>

      <BodyScrollView testID="SymbolBarScroll" keyboardShouldPersistTaps="handled" contentContainerStyle={colors && styles.body}>
        <YStack gap={colors ? 26 : "$4"} pb="$6">
          <YStack gap={colors ? 8 : "$2"}>
            <XStack
              items="center"
              justify="space-between"
              px={colors ? 21 : "$4"}
              py={colors ? 0 : "$3"}
              height={colors ? 56 : undefined}
              rounded={colors ? 28 : "$5"}
              bg="$color3"
              style={colors && { backgroundColor: colors.group }}
            >
              <Text color={colors?.label ?? "$color"} fontSize={colors ? 18 : undefined}>Show Symbol Bar</Text>
              <View style={colors && styles.switchFrame}>
                <Switch
                  testID="SymbolBarSwitch"
                  accessibilityLabel="Show Symbol Bar"
                  value={shown}
                  onValueChange={setShown}
                  trackColor={colors && { true: '#24cc46' }}
                />
              </View>
            </XStack>
            <Text variant="muted" px={colors ? 21 : "$2"} style={colors && [styles.caption, { color: colors.caption }]} testID="SymbolBarSwitchCaption">
              Shown above the keyboard while editing code.
            </Text>
          </YStack>

          <YStack gap={colors ? 8 : "$2"}>
            <YStack rounded={colors ? 28 : "$5"} bg="$color3" style={colors && { backgroundColor: colors.group }} overflow={colors ? 'visible' : 'hidden'} testID="SymbolBarGroups">
              <ReorderableList
                data={groups.slice()}
                keyExtractor={keyOf}
                rowHeight={ROW_HEIGHT}
                activation="immediate"
                handleIcon={Menu}
                handleIconColor={colors?.handle}
                handleSystemSymbol={colors ? 'line.3.horizontal' : undefined}
                liftedRowStyle={colors && LIFTED_ROW[scheme === 'dark' ? 'dark' : 'light']}
                onReorder={onReorder}
                renderItem={(group, handle) => (
                  <GroupRow
                    group={group}
                    index={indexOf(group)}
                    last={group.id === groups.at(-1)?.id}
                    colors={colors}
                    handle={handle}
                  />
                )}
              />
            </YStack>
            <Text variant="muted" px={colors ? 21 : "$2"} style={colors && [styles.caption, { color: colors.caption }]} testID="SymbolBarGroupsCaption">
              Each group holds up to 4 characters. Leave a group empty to hide it. Drag the handle to reorder.
            </Text>
          </YStack>

          {showRailStyle ? <YStack gap="$2" testID="SymbolBarRailStyleRow">
            <XStack gap="$2" role="radiogroup" aria-label="Which keys sit above the keyboard" testID="SymbolBarRailStyle">
              {RAIL_STYLES.map((style) => (
                <Button
                  key={style}
                  flex={1}
                  size="$3"
                  variant={railStyle === style ? 'default' : 'outline'}
                  role="radio"
                  aria-checked={railStyle === style}
                  pressLog="off"
                  testID={`SymbolBarRailStyle-${style}`}
                  onPress={() => {
                    Logger.info('leetcode.symbol_bar.rail_style_tap', { railStyle: style });
                    if (railStyle !== style) setRailStyle(style);
                  }}
                >
                  {RAIL_STYLE_LABELS[style]}
                </Button>
              ))}
            </XStack>
            <Text variant="muted" px="$2">
              Symbols shows Tab and these groups; Classic shows the character keys and its own toolbar settings.
            </Text>
          </YStack> : null}
        </YStack>
      </BodyScrollView>
    </>
  );

  return nativePage ? (
    <PageSheet open={open} onOpenChange={onOpenChange} onDismiss={onDismiss} testID={testID} contentStyle={colors && { ...styles.page, backgroundColor: colors.page }}>
      {content}
    </PageSheet>
  ) : (
    <Sheet open={open} onOpenChange={onOpenChange} snapPoints={[92]} disableContentPanningGesture testID={testID}>
      {content}
    </Sheet>
  );
}


const styles = StyleSheet.create({
  page: { paddingHorizontal: 20, paddingTop: 7 },
  header: { borderBottomWidth: 0, paddingBottom: 0 },
  body: { paddingTop: 48 },
  caption: { fontSize: 13.5, lineHeight: 19 },
  switchFrame: { height: 28, justifyContent: 'center' },
  divider: { position: 'absolute', bottom: 0, left: 20, right: 20, height: 1 },
});
