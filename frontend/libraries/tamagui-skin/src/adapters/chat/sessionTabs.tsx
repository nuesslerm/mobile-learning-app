import type { ChatImageProps, SessionTabsSheetProps } from '@lib/skin';
import { Icon, InsetGroup, ListItem, Text } from '@lib/tamagui-design-system';
import { Check, SquareTerminal, X } from '@lib/tamagui-design-system/icons';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';
import { Image } from 'react-native';

import { ScreenSheet } from '../sheet/ScreenSheet';

/** The open tabs of a workspace as a sheet of list rows: the picked one ticked, a closeable one with its own close button. */
export function SessionTabsSheet(props: SessionTabsSheetProps) {
  if (!props.visible) return null;
  const title = props.sessions.length === 1 ? '1 tab' : `${props.sessions.length} tabs`;
  return (
    <ScreenSheet onClose={props.onClose} snapPoints={[60]} handleLabel="Close" testID="skin-session-tabs">
      <YStack gap="$3" pb={Math.max(16, props.bottomInset)}>
        <YStack px="$4" gap="$1">
          <Text variant="large" role="heading">
            {title}
          </Text>
          <Text variant="muted" numberOfLines={1}>
            {props.workspace}
          </Text>
        </YStack>
        <InsetGroup testID="skin-session-tabs-list">
          {props.sessions.map((tab) => (
            <XStack key={tab.id} items="center">
              <YStack flex={1}>
                <ListItem
                  variant="grouped"
                  pressLog="off"
                  accessibilityLabel={tab.title}
                  selected={tab.id === props.activeId}
                  onPress={() => {
                    props.onClose();
                    props.onSelect(tab.id);
                  }}
                  testID="skin-session-tab"
                >
                  <XStack items="center" gap="$3">
                    <Icon as={SquareTerminal} size={17} color="$color11" />
                    <Text flex={1} numberOfLines={1}>
                      {tab.title}
                    </Text>
                    {tab.id === props.activeId ? <Icon as={Check} size={17} color="$blue10" /> : null}
                  </XStack>
                </ListItem>
              </YStack>
              {props.closeableIds?.includes(tab.id) ? (
                <XStack role="button" aria-label={`Close ${tab.title}`} p="$3" pressStyle={{ opacity: 0.7 }} cursor="pointer" onPress={() => props.onCloseTab?.(tab.id)}>
                  <Icon as={X} size={16} color="$color11" />
                </XStack>
              ) : null}
            </XStack>
          ))}
        </InsetGroup>
        {props.error ? (
          <XStack role="button" items="center" justify="center" gap="$2" px="$4" pressStyle={{ opacity: 0.7 }} cursor="pointer" onPress={props.onRetry}>
            <Text variant="muted">{props.error}</Text>
            <Text color="$blue10">Try again</Text>
          </XStack>
        ) : null}
      </YStack>
    </ScreenSheet>
  );
}

const THUMB = { width: 160, height: 120, borderRadius: 12 } as const;

/** An attached image in a message: its thumbnail, which opens it full size. */
export function ChatImage({ uri, label, onOpen, unavailable }: ChatImageProps) {
  if (unavailable) return <Text variant="muted">🖼 {label}</Text>;
  if (!uri) return <YStack aria-label={label} width={THUMB.width} height={THUMB.height} rounded={THUMB.borderRadius} bg="$color3" borderWidth={1} borderColor="$borderColor" />;
  return (
    <YStack role="button" aria-label={label} self="flex-start" rounded={THUMB.borderRadius} overflow="hidden" bg="$color3" borderWidth={1} borderColor="$borderColor" pressStyle={{ opacity: 0.6 }} cursor="pointer" onPress={onOpen}>
      <Image source={{ uri }} resizeMode="contain" style={THUMB} />
    </YStack>
  );
}
