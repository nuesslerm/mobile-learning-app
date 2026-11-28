import type { HostWorkspaceListProps } from '@lib/skin';
import { Button, Icon, IconButton, Input, InsetGroup, Text } from '@lib/tamagui-design-system';
import { Bell, ChevronDown, ChevronLeft, ChevronRight, Folder, Pin, Search } from '@lib/tamagui-design-system/icons';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';
import { ScrollView } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { longPress } from '../longPress';

type Section = HostWorkspaceListProps['sections'][number];
type Row = Section['rows'][number];

const CONTENT = { paddingBottom: 16, gap: 12 } as const;

function WorkspaceRow({ row, onSelect, onActions }: { row: Row; onSelect: () => void; onActions?: () => void }) {
  return (
    <XStack
      role="button"
      accessible
      aria-label={row.name}
      items="center"
      gap="$3"
      px="$4"
      py="$3"
      minH={52}
      pressStyle={{ bg: '$color3' }}
      cursor="pointer"
      onPress={onSelect}
      {...longPress(onActions, 'Workspace actions')}
    >
      <YStack width={10} items="center" gap="$1.5">
        <YStack width={8} height={8} rounded={4} bg="$color8" />
        {row.unread ? <Icon as={Bell} size={11} color="$yellow9" filled /> : null}
      </YStack>
      <YStack flex={1} gap="$1">
        <XStack items="center" gap="$2">
          {row.projectInTitle ? <Icon as={Folder} size={12} color="$color10" /> : null}
          <Text variant="strong" numberOfLines={1} shrink={1}>
            {row.name}
          </Text>
        </XStack>
        {row.project ? (
          <XStack items="center" gap="$1.5">
            <Icon as={Folder} size={12} color="$color10" />
            <Text variant="muted" numberOfLines={1} maxW={120}>
              {row.project}
            </Text>
            {row.detail ? (
              <Text variant="code" color="$color10" numberOfLines={1} flex={1}>
                {row.detail}
              </Text>
            ) : null}
          </XStack>
        ) : row.detail ? (
          <Text variant="code" color="$color11" numberOfLines={1}>
            {row.detail}
          </Text>
        ) : null}
      </YStack>
    </XStack>
  );
}

/** A desktop's workspaces as the app's grouped lists: a heading that folds its group, one card of rows under it, the search at the foot. */
export function HostWorkspaceList(props: HostWorkspaceListProps) {
  const { top, bottom } = useSafeAreaInsets();
  const folded = (key: string) => props.disclosure?.keys.has(key) ?? false;
  const ready = Boolean(props.disclosure?.ready);
  return (
    <YStack flex={1} bg="$background" pt={top} pb={bottom}>
      <XStack px="$3" minH={52} items="center">
        <IconButton icon={ChevronLeft} variant="secondary" glass={false} size="$3" accessibilityLabel="Back" pressLog="off" onPress={props.onBack} testID="skin-workspaces-back" />
      </XStack>
      <ScrollView keyboardShouldPersistTaps="handled" contentContainerStyle={CONTENT}>
        <YStack px="$4" gap="$1">
          <Text variant="h3" role="heading">
            {props.title}
          </Text>
          {props.platform ? (
            <XStack items="center" gap="$2">
              <YStack width={8} height={8} rounded={4} bg={props.offline ? '$yellow9' : '$green9'} />
              <Text variant="muted">{props.platform}</Text>
            </XStack>
          ) : null}
        </YStack>
        {props.availability}
        {props.sections.map(({ key, label, icon, rows }) => (
          <YStack key={key} gap="$1">
            {label ? (
              <XStack
                role="button"
                accessible
                aria-label={`${label} ${rows.length}`}
                aria-expanded={!folded(key)}
                aria-disabled={!ready}
                disabled={!ready}
                items="center"
                gap="$2"
                mx="$4"
                minH={40}
                pressStyle={{ opacity: 0.7 }}
                cursor="pointer"
                onPress={() => props.disclosure?.change(key, !folded(key))}
              >
                <Icon as={folded(key) ? ChevronRight : ChevronDown} size={14} color="$color10" />
                {icon === 'pin' ? <Icon as={Pin} size={13} color="$color10" /> : null}
                {icon === 'folder' ? <Icon as={Folder} size={15} color="$color10" /> : null}
                <Text variant="small" color="$color11">
                  {label}
                </Text>
                <Text variant="small" color="$color10">
                  {rows.length}
                </Text>
              </XStack>
            ) : null}
            {(!label || !folded(key)) && rows.length > 0 ? (
              <InsetGroup testID="skin-workspaces-rows">
                {rows.map((row) => (
                  <WorkspaceRow key={row.id} row={row} onSelect={() => props.onSelect(row.id)} onActions={props.onActions && (() => props.onActions?.(row.id))} />
                ))}
              </InsetGroup>
            ) : null}
          </YStack>
        ))}
        {props.notice ? (
          <Text aria-live="polite" color="$color11" mx="$4">
            {props.notice}
          </Text>
        ) : null}
        {props.retry ? (
          <Button variant="ghost" glass={false} self="flex-start" mx="$2" pressLog="off" onPress={props.retry} testID="skin-workspaces-retry">
            Try again
          </Button>
        ) : null}
      </ScrollView>
      {props.search ? (
        <XStack items="center" gap="$2" px="$3" py="$2">
          {props.options}
          <XStack flex={1} items="center" gap="$2" px="$3" rounded="$10" bg="$color3">
            <Icon as={Search} size={16} color="$color10" />
            <Input
              unstyled
              flex={1}
              height={44}
              fontSize={16}
              color="$color"
              value={props.search.value}
              onChangeText={props.search.onChange}
              placeholder="Search workspaces"
              placeholderTextColor="$color10"
              aria-label="Search workspaces"
              autoCapitalize="none"
              autoCorrect={false}
              testID="skin-workspaces-search"
            />
          </XStack>
        </XStack>
      ) : null}
    </YStack>
  );
}
