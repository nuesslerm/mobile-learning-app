import { sessionStatus, tabCountLabel, useSessionMenu, useTabStrip, type SessionHeaderProps } from '@lib/skin';
import { Button, Icon, IconButton, Text } from '@lib/tamagui-design-system';
import { ChevronLeft, Ellipsis, FileText, GitBranch, Globe, MessageSquare, Plus, SquareTerminal } from '@lib/tamagui-design-system/icons';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';
import type { LucideIcon } from 'lucide-react-native';
import { useState } from 'react';
import { ScrollView } from 'react-native';

import { ScreenSheet } from '../sheet/ScreenSheet';
import { SELECTABLE, SessionMenuSheet } from './sessionFiles';

const TAB_ICONS: Record<string, LucideIcon> = { browser: Globe, markdown: FileText, terminal: SquareTerminal };
const FLANK_WIDTH = 84;
const STRIP = { alignItems: 'center', paddingHorizontal: 8, gap: 6 } as const;

/** The top of a session: where it runs and whether the desktop answers, the ways out of it, and its tabs as a strip of pills. */
export function SessionHeader(props: SessionHeaderProps) {
  const { strip, reveal, place } = useTabStrip(props.activeId);
  const menu = useSessionMenu();
  const [details, setDetails] = useState(false);
  return (
    <>
      <YStack testID="MobileT3Header" pt={props.topInset} bg="$background" borderBottomWidth={1} borderColor="$borderColor">
        <XStack items="center" px="$3" minH={52}>
          <XStack width={FLANK_WIDTH}>
            <IconButton
              icon={ChevronLeft}
              variant="secondary"
              glass={false}
              size="$3"
              accessibilityLabel="Back to worktrees"
              pressLog="off"
              onPress={props.onBack}
              testID="skin-session-back"
            />
          </XStack>
          <YStack flex={1} items="center">
            <YStack
              role="button"
              aria-label={props.workspace}
              aria-expanded={details}
              items="center"
              maxW="100%"
              pressStyle={{ opacity: 0.7 }}
              cursor="pointer"
              onPress={() => setDetails(true)}
            >
              <Text variant="strong" numberOfLines={1}>
                {props.workspace}
              </Text>
              <XStack items="center" gap="$2">
                <YStack width={8} height={8} rounded={4} bg={props.connected ? '$green9' : '$yellow9'} />
                <Text variant="muted" numberOfLines={1} shrink={1}>
                  {sessionStatus(props.connected, props.branch)}
                </Text>
              </XStack>
            </YStack>
            {props.connected ? null : (
              <YStack role="button" aria-label="Reconnect to desktop" position="absolute" l={0} r={0} b={0} height={18} cursor="pointer" onPress={props.onReconnect} />
            )}
          </YStack>
          <XStack width={FLANK_WIDTH} justify="flex-end" gap="$2">
            {props.onOpenSourceControl ? (
              <IconButton
                icon={GitBranch}
                variant="secondary"
                glass={false}
                size="$3"
                accessibilityLabel="Open source control"
                pressLog="off"
                onPress={props.onOpenSourceControl}
                testID="skin-session-source-control"
              />
            ) : null}
            <IconButton
              icon={Ellipsis}
              variant="secondary"
              glass={false}
              size="$3"
              accessibilityLabel="More actions"
              pressLog="off"
              onPress={() => menu.setPage('menu')}
              testID="skin-session-more"
            />
          </XStack>
        </XStack>
        <XStack height={44} items="center" borderTopWidth={1} borderColor="$borderColor">
          {props.tabs ? (
            <ScrollView
              ref={strip}
              onContentSizeChange={reveal}
              horizontal
              showsHorizontalScrollIndicator={false}
              keyboardShouldPersistTaps="always"
              style={{ flex: 1 }}
              contentContainerStyle={STRIP}
            >
              {props.tabs.map((tab) => {
                const selected = tab.id === props.activeId;
                const ink = selected ? '$color' : '$color10';
                return (
                  <XStack
                    key={tab.id}
                    role="tab"
                    aria-label={tab.title}
                    aria-selected={selected}
                    onLayout={(event) => place(tab.id, event.nativeEvent.layout.x)}
                    height={32}
                    maxW={160}
                    px="$3"
                    gap="$1.5"
                    items="center"
                    rounded={9999}
                    bg={selected ? '$color4' : 'transparent'}
                    pressStyle={{ opacity: 0.7 }}
                    cursor="pointer"
                    onPress={() => props.onSelectTab?.(tab.id)}
                    onLongPress={props.onShowTabs}
                  >
                    <Icon as={TAB_ICONS[tab.kind ?? ''] ?? MessageSquare} size={14} color={ink} />
                    <Text fontSize={13} numberOfLines={1} shrink={1} color={ink}>
                      {tab.title}
                    </Text>
                  </XStack>
                );
              })}
            </ScrollView>
          ) : (
            <>
              <XStack flex={1} minW={0} items="center" gap="$2" px="$4">
                <Icon as={SquareTerminal} size={15} color="$color10" />
                <Text variant="strong" numberOfLines={1} shrink={1}>
                  {props.title}
                </Text>
              </XStack>
              <Button
                variant="secondary"
                glass={false}
                size="$2"
                accessibilityLabel={tabCountLabel(props.tabCount)}
                pressLog="off"
                onPress={props.onShowTabs}
                testID="skin-session-tab-count"
              >
                {String(props.tabCount)}
              </Button>
            </>
          )}
          {props.onNewTab ? (
            <IconButton
              icon={Plus}
              variant="secondary"
              glass={false}
              size="$3"
              mx="$2"
              accessibilityLabel="New Tab"
              pressLog="off"
              onPress={props.onNewTab}
              testID="skin-session-new-tab"
            />
          ) : null}
        </XStack>
      </YStack>
      {details ? (
        <ScreenSheet onClose={() => setDetails(false)} snapPoints={[30]} handleLabel="Close workspace details" testID="skin-session-workspace">
          <YStack role="dialog" aria-label={props.workspace} px="$4" gap="$1">
            <Text variant="large" {...SELECTABLE}>
              {props.workspace}
            </Text>
            {props.directory ? (
              <Text variant="muted" {...SELECTABLE}>
                {props.directory}
              </Text>
            ) : null}
          </YStack>
        </ScreenSheet>
      ) : null}
      <SessionMenuSheet menu={menu} files={props.files} workspace={props.workspace} onOpenTextSettings={props.onOpenTextSettings} />
    </>
  );
}
