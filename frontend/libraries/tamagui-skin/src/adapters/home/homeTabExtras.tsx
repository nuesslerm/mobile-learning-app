import type { HomeActionSheetProps, HomeAgentSectionProps, HomeEmptyStateProps, HomeSearchFieldProps } from '@lib/skin';
import { Icon, Input, InsetGroup, ListItem, Text } from '@lib/tamagui-design-system';
import { Bell, Check, CircleX, Search } from '@lib/tamagui-design-system/icons';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';

import { ScreenSheet } from '../sheet/ScreenSheet';
import { homeIcons } from './homeIcons';

/** What a tab shows when it has nothing to show; the sizes a caller passes are the other skin's. */
export function HomeEmptyState(props: HomeEmptyStateProps) {
  return (
    <YStack items="center" px="$6" py="$10" gap="$2" testID="skin-home-empty">
      <Icon as={homeIcons[props.icon]} size={32} color="$color10" />
      <Text variant="large" role="heading" text="center">
        {props.title}
      </Text>
      {props.message ? (
        <Text variant="muted" text="center">
          {props.message}
        </Text>
      ) : null}
    </YStack>
  );
}

export function HomeSearchField(props: HomeSearchFieldProps) {
  return (
    <XStack items="center" gap="$2" mx="$4" my="$2" px="$3" rounded="$4" bg="$color3">
      <Icon as={Search} size={16} color="$color10" />
      <Input
        unstyled
        flex={1}
        height={40}
        fontSize={16}
        color="$color"
        value={props.value}
        onChangeText={props.onChangeText}
        placeholder={props.placeholder}
        placeholderTextColor="$color10"
        aria-label={props.placeholder}
        autoCapitalize="none"
        autoCorrect={false}
        autoFocus
        testID="HomeSearchField"
      />
      {props.value.length > 0 ? (
        <XStack role="button" aria-label="Clear search" p="$1" pressStyle={{ opacity: 0.7 }} cursor="pointer" onPress={props.onClear}>
          <Icon as={CircleX} size={16} color="$color10" />
        </XStack>
      ) : null}
    </XStack>
  );
}

const STATUS_INK = {
  interrupted: '$color10',
  done: '$green9',
  working: '$yellow9',
  waiting: '$red9',
  blocked: '$red9',
  failed: '$red9',
} as const;

export function HomeAgentSection(props: HomeAgentSectionProps) {
  return (
    <YStack gap="$2" pb="$3">
      <Text variant="muted" role="heading" textTransform="uppercase" letterSpacing={0.5} mx="$4">
        {props.title} {props.rows.length}
      </Text>
      <InsetGroup testID="skin-home-agents">
        {props.rows.map((row) => (
          <ListItem
            key={row.id}
            variant="grouped"
            pressLog="off"
            accessibilityLabel={`${row.title}, ${row.place}`}
            onPress={() => props.onSelect(row.id)}
            testID="skin-home-agent"
          >
            <XStack items="center" gap="$3">
              <YStack width={8} height={8} rounded={4} bg={STATUS_INK[row.status]} />
              <YStack flex={1} gap="$1">
                <XStack items="baseline" gap="$2">
                  <Text flex={1} numberOfLines={1} variant={row.unread ? 'strong' : 'default'}>
                    {row.title}
                  </Text>
                  <Text variant="muted">{row.age}</Text>
                </XStack>
                <XStack items="center" gap="$2">
                  <Text variant="muted" flex={1} numberOfLines={1}>
                    {row.place}
                  </Text>
                  {row.unread ? <Icon as={Bell} size={12} color="$yellow9" filled /> : null}
                </XStack>
              </YStack>
            </XStack>
          </ListItem>
        ))}
      </InsetGroup>
    </YStack>
  );
}

export function HomeActionSheet(props: HomeActionSheetProps) {
  if (!props.visible) return null;
  return (
    <ScreenSheet onClose={props.onClose} handleLabel="Close" testID="skin-home-actions">
      <YStack gap="$3" pb="$4">
        {props.groups.map((group, index) => (
          <InsetGroup key={index} testID="skin-home-actions-group">
            {group.map((action) => (
              <ListItem
                key={action.id}
                variant="grouped"
                pressLog="off"
                accessibilityLabel={action.label}
                selected={action.checked}
                disabled={action.disabled}
                onPress={action.onPress}
                testID={`skin-home-action-${action.id}`}
              >
                <XStack items="center" gap="$3">
                  <Text flex={1}>{action.label}</Text>
                  {action.checked ? <Icon as={Check} size={18} /> : null}
                </XStack>
              </ListItem>
            ))}
          </InsetGroup>
        ))}
      </YStack>
    </ScreenSheet>
  );
}
