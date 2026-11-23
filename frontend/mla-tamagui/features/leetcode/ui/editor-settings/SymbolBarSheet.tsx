import { Logger } from '@lib/logging';
import { Button, Input, Switch, Text } from '@lib/tamagui-design-system';
import { Menu } from '@lib/tamagui-design-system/icons';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';
import { Sheet, SheetHeader } from '@lib/tamagui-design-system/ui/sheet';
import { useCallback, type ReactNode } from 'react';

import {
  groupLabel,
  isDefaultGroups,
  moveBetween,
  type SymbolGroup,
} from '../../domain/editor-settings/symbolGroups';
import {
  RAIL_STYLES,
  useSymbolBarStore,
  type RailStyle,
} from '../../state/editor/symbolBarStore';
import { ReorderableList } from '../ReorderableList';

const ROW_HEIGHT = 56;
const CHARS_STYLE = { fontFamily: 'Menlo, Courier, monospace', textAlign: 'right' } as const;

const RAIL_STYLE_LABELS: Record<RailStyle, string> = {
  classic: 'Classic',
  symbols: 'Symbols',
};

type Props = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  testID?: string;
};

function GroupRow({ group, index, handle }: { group: SymbolGroup; index: number; handle: ReactNode }) {
  const setGroup = useSymbolBarStore((s) => s.setGroup);
  return (
    <XStack height={ROW_HEIGHT} items="center" gap="$3" pl="$4" testID={`SymbolBarRow-${group.id}`}>
      <Text flex={1} testID={`SymbolBarRowLabel-${group.id}`}>
        {groupLabel(index)}
      </Text>
      <Input
        testID={`SymbolBarChars-${group.id}`}
        aria-label={`${groupLabel(index)} characters`}
        value={group.chars}
        width={104}
        autoCapitalize="none"
        autoCorrect={false}
        borderWidth={0}
        bg="transparent"
        style={CHARS_STYLE}
        onChangeText={(text) => setGroup(group.id, text)}
      />
      {handle}
    </XStack>
  );
}

/**
 * The recording's Symbol Bar sheet: the switch, the eight groups (each edited in place and reordered by its handle) and Reset and Done. It reads and writes only the symbol bar store; the caller owns whether it is open.
 * The Classic | Symbols control is temporary: it goes when the symbols rail becomes the default.
 */
export function SymbolBarSheet({ open, onOpenChange, testID = 'SymbolBarSheet' }: Props) {
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

  return (
    <Sheet open={open} onOpenChange={onOpenChange} snapPoints={[92]} disableContentPanningGesture testID={testID}>
      <SheetHeader
        testID={`${testID}Header`}
        leading={
          <Button
            testID="SymbolBarReset"
            variant="outline"
            size="$3"
            rounded="$10"
            disabled={isDefaultGroups(groups)}
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
            variant="default"
            size="$3"
            rounded="$10"
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
        <Text variant="h4" text="center" testID="SymbolBarTitle">
          Symbol Bar
        </Text>
      </SheetHeader>

      <Sheet.ScrollView testID="SymbolBarScroll">
        <YStack gap="$4" pb="$6">
          <YStack gap="$2">
            <XStack items="center" justify="space-between" px="$4" py="$3" rounded="$5" bg="$color3">
              <Text>Show Symbol Bar</Text>
              <Switch testID="SymbolBarSwitch" accessibilityLabel="Show Symbol Bar" value={shown} onValueChange={setShown} />
            </XStack>
            <Text variant="muted" px="$2" testID="SymbolBarSwitchCaption">
              Shown above the keyboard while editing code.
            </Text>
          </YStack>

          <YStack gap="$2">
            <YStack rounded="$5" bg="$color3" overflow="hidden" testID="SymbolBarGroups">
              <ReorderableList
                data={groups.slice()}
                keyExtractor={keyOf}
                rowHeight={ROW_HEIGHT}
                activation="immediate"
                handleIcon={Menu}
                onReorder={onReorder}
                renderItem={(group, handle) => <GroupRow group={group} index={indexOf(group)} handle={handle} />}
              />
            </YStack>
            <Text variant="muted" px="$2" testID="SymbolBarGroupsCaption">
              Each group holds up to 4 characters. Leave a group empty to hide it. Drag the handle to reorder.
            </Text>
          </YStack>

          <YStack gap="$2" testID="SymbolBarRailStyleRow">
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
              Classic keeps today's keys; Symbols shows Tab and these groups.
            </Text>
          </YStack>
        </YStack>
      </Sheet.ScrollView>
    </Sheet>
  );
}
