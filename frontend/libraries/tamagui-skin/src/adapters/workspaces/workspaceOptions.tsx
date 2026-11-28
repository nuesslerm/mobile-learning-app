import type { WorkspaceOptionGroup, WorkspaceOptionRow, WorkspaceOptionsProps } from '@lib/skin';
import { Button, Icon, IconButton, InsetGroup, ListItem, Switch, Text } from '@lib/tamagui-design-system';
import { Check, ChevronLeft, SlidersHorizontal } from '@lib/tamagui-design-system/icons';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';
import { Sheet } from '@lib/tamagui-design-system/ui/sheet';

import { ScreenSheet } from '../sheet/ScreenSheet';

const TALL = [80];
const FLANK_WIDTH = 76;
const CONTENT = { gap: 12, paddingBottom: 24 } as const;

function RowText({ row }: { row: WorkspaceOptionRow }) {
  return (
    <>
      <YStack flex={1} gap="$1">
        <Text variant={row.description ? 'strong' : 'default'}>{row.label}</Text>
        {row.description ? <Text variant="muted">{row.description}</Text> : null}
      </YStack>
      {row.value === undefined ? null : <Text color="$color11">{row.value}</Text>}
    </>
  );
}

function OptionRow({ row, disabled }: { row: WorkspaceOptionRow; disabled: boolean }) {
  const label = row.value === undefined ? row.label : `${row.label}, ${row.value}`;
  if (row.kind === 'page')
    return (
      <ListItem variant="grouped" showChevron pressLog="off" accessibilityLabel={label} onPress={row.onPress} testID="skin-workspace-option-page">
        <XStack items="center" gap="$3">
          <RowText row={row} />
        </XStack>
      </ListItem>
    );
  if (row.kind === 'switch')
    return (
      <XStack items="center" gap="$3" px="$4" py="$2.5">
        <RowText row={row} />
        <Switch value={Boolean(row.selected)} onValueChange={row.onPress} accessibilityLabel={row.label} disabled={disabled} testID="skin-workspace-option-switch" />
      </XStack>
    );
  return (
    <XStack
      role={row.kind ?? 'button'}
      accessible
      aria-label={label}
      aria-checked={row.selected}
      aria-disabled={disabled}
      disabled={disabled}
      items="center"
      gap="$3"
      px="$4"
      py="$3"
      pressStyle={{ bg: '$color3' }}
      cursor="pointer"
      onPress={row.onPress}
    >
      <RowText row={row} />
      {row.selected ? <Icon as={Check} size={17} /> : null}
    </XStack>
  );
}

function OptionGroup({ group, disabled }: { group: WorkspaceOptionGroup; disabled: boolean }) {
  return (
    <YStack gap="$2">
      {group.label ? (
        <Text variant="muted" textTransform="uppercase" letterSpacing={0.5} mx="$4">
          {group.label}
        </Text>
      ) : null}
      {group.segments ? (
        <XStack mx="$4" p="$1" gap="$1" rounded="$4" bg="$color3">
          {group.rows.map((row) => (
            <XStack
              key={row.label}
              role="radio"
              accessible
              aria-label={row.label}
              aria-checked={Boolean(row.selected)}
              aria-disabled={disabled}
              disabled={disabled}
              flex={1}
              minH={34}
              items="center"
              justify="center"
              rounded="$3"
              bg={row.selected ? '$background' : 'transparent'}
              pressStyle={{ opacity: 0.7 }}
              cursor="pointer"
              onPress={row.onPress}
            >
              <Text variant="small" numberOfLines={1} color={row.selected ? '$color' : '$color11'}>
                {row.label}
              </Text>
            </XStack>
          ))}
        </XStack>
      ) : (
        <InsetGroup testID="skin-workspace-option-group">
          {group.rows.map((row) => (
            <OptionRow key={row.label} row={row} disabled={disabled} />
          ))}
        </InsetGroup>
      )}
    </YStack>
  );
}

/** How a desktop's workspaces are sorted, grouped and shown: a button that opens a sheet of grouped choices, with pages for the longer ones. */
export function WorkspaceOptions(props: WorkspaceOptionsProps) {
  const groups = props.page ? props.pageGroups : props.groups;
  return (
    <>
      <IconButton icon={SlidersHorizontal} variant="secondary" glass={false} size="$4" accessibilityLabel="Workspace options" pressLog="off" onPress={props.onOpen} testID="skin-workspace-options" />
      {props.visible ? (
        <ScreenSheet onClose={props.onClose} snapPoints={TALL} handleLabel="Close" testID="skin-workspace-options-sheet">
          <YStack flex={1} gap="$3">
            <XStack items="center">
              <XStack width={FLANK_WIDTH}>
                {props.page ? (
                  <IconButton icon={ChevronLeft} variant="secondary" glass={false} size="$3" accessibilityLabel="Back to Workspace options" pressLog="off" onPress={props.onBack} testID="skin-workspace-options-back" />
                ) : (
                  <Button variant="ghost" glass={false} size="$3" disabled={!props.resetEnabled || props.saving} pressLog="off" onPress={props.onReset} testID="skin-workspace-options-reset">
                    Reset
                  </Button>
                )}
              </XStack>
              <YStack flex={1} items="center">
                <Text variant="strong" role="heading" numberOfLines={1}>
                  {props.page ?? 'Workspace options'}
                </Text>
                {props.page ? null : <Text variant="muted">Synced across your devices</Text>}
              </YStack>
              <XStack width={FLANK_WIDTH} justify="flex-end">
                <Button variant="ghost" glass={false} size="$3" pressLog="off" onPress={props.onClose} testID="skin-workspace-options-done">
                  Done
                </Button>
              </XStack>
            </XStack>
            <Sheet.ScrollView key={props.page ?? 'root'} bounces={false} contentContainerStyle={CONTENT}>
              {props.loading ? <Text mx="$4">Loading workspace options…</Text> : null}
              {props.error ? (
                <YStack gap="$1">
                  <Text role="alert" mx="$4">
                    {props.error}
                  </Text>
                  <Button variant="ghost" glass={false} self="flex-start" mx="$2" pressLog="off" onPress={props.onRetry} testID="skin-workspace-options-retry">
                    Try again
                  </Button>
                </YStack>
              ) : null}
              {groups.map((group) => (
                <OptionGroup key={group.label ?? group.rows[0]?.label} group={group} disabled={props.saving} />
              ))}
            </Sheet.ScrollView>
          </YStack>
        </ScreenSheet>
      ) : null}
    </>
  );
}
