import {
  fileName,
  useFilePreview,
  useFileTree,
  useSkin,
  type FileTreeRow,
  type SessionFileSource,
  type SessionHeaderProps,
  type SessionMenuPage,
  type useSessionMenu,
} from '@lib/skin';
import { Button, Icon, IconButton, InsetGroup, ListItem, Spinner, Text } from '@lib/tamagui-design-system';
import { ChevronDown, ChevronLeft, ChevronRight, FileText, Folder, FolderOpen, Type } from '@lib/tamagui-design-system/icons';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';
import { Sheet } from '@lib/tamagui-design-system/ui/sheet';
import type { ReactNode } from 'react';
import { Platform } from 'react-native';

import { ScreenSheet } from '../sheet/ScreenSheet';

const SNAP_POINTS: Record<SessionMenuPage, number[]> = { menu: [28], text: [48], files: [92] };
const ROW_HEIGHT = 44;
const LIST_END = { paddingBottom: 24 } as const;
// react-native-web's Text takes `selectable`, but the design system's Text renders a plain DOM node on web
export const SELECTABLE = Platform.OS === 'web' ? { userSelect: 'text' as const } : { selectable: true };

function Centred({ children }: { children: ReactNode }) {
  return (
    <YStack flex={1} items="center" justify="center" gap="$2">
      {children}
    </YStack>
  );
}

function FileTree({ source, onRead }: { source: SessionFileSource; onRead: (path: string) => void }) {
  const { loading, rows, expanded, load, toggle } = useFileTree(source);
  if (loading)
    return (
      <Centred>
        <Spinner />
        <Text variant="muted">Loading files…</Text>
      </Centred>
    );
  return (
    <Sheet.FlatList
      data={rows}
      keyExtractor={(row: FileTreeRow) => ('entry' in row ? row.entry.path : `status:${row.parent}`)}
      contentContainerStyle={LIST_END}
      renderItem={({ item }: { item: FileTreeRow }) => {
        const indent = 8 + item.depth * 16;
        if ('status' in item)
          return (
            <XStack
              minH={ROW_HEIGHT}
              items="center"
              pl={indent}
              pr="$3"
              {...(item.retry ? { role: 'button', cursor: 'pointer', pressStyle: { opacity: 0.7 }, onPress: () => load(item.parent) } : {})}
            >
              <Text variant="muted">{item.status}</Text>
            </XStack>
          );
        const { entry } = item;
        const name = fileName(entry.path);
        const directory = entry.kind === 'directory';
        const open = expanded.has(entry.path);
        return (
          <XStack
            role="button"
            aria-label={name}
            aria-expanded={directory ? open : undefined}
            minH={ROW_HEIGHT}
            items="center"
            gap="$2"
            pl={indent}
            pr="$3"
            rounded="$3"
            pressStyle={{ bg: '$color3' }}
            cursor="pointer"
            onPress={() => (directory ? toggle(entry.path) : onRead(entry.path))}
          >
            {directory ? <Icon as={open ? ChevronDown : ChevronRight} size={16} color="$color10" /> : <XStack width={16} />}
            <Icon as={directory ? (open ? FolderOpen : Folder) : FileText} size={17} color="$color11" />
            <Text flex={1} numberOfLines={1}>
              {name}
            </Text>
          </XStack>
        );
      }}
    />
  );
}

function FilePreview({ source, path }: { source: SessionFileSource; path: string }) {
  const { file, failed, retry } = useFilePreview(source, path);
  if (failed)
    return (
      <YStack flex={1} items="center" justify="center" role="button" cursor="pointer" pressStyle={{ opacity: 0.7 }} onPress={retry}>
        <Text>Could not read this file. Try again</Text>
      </YStack>
    );
  if (!file)
    return (
      <Centred>
        <Spinner />
      </Centred>
    );
  return (
    <Sheet.ScrollView contentContainerStyle={LIST_END}>
      {file.truncated ? <Text variant="muted">Showing the first 1 MiB</Text> : null}
      <Text variant="code" {...SELECTABLE}>
        {file.contents}
      </Text>
    </Sheet.ScrollView>
  );
}

type Menu = ReturnType<typeof useSessionMenu>;
type Props = Pick<SessionHeaderProps, 'files' | 'workspace' | 'onOpenTextSettings'> & { menu: Menu };

function MenuList({ menu }: { menu: Menu }) {
  return (
    <InsetGroup testID="skin-session-menu-list">
      <ListItem variant="grouped" showChevron pressLog="off" accessibilityLabel="File explorer" onPress={() => menu.setPage('files')} testID="skin-session-menu-files">
        <XStack items="center" gap="$3">
          <Icon as={Folder} size={18} color="$color11" />
          <Text flex={1}>File explorer</Text>
        </XStack>
      </ListItem>
      <ListItem variant="grouped" showChevron pressLog="off" accessibilityLabel="Text size" onPress={() => menu.setPage('text')} testID="skin-session-menu-text">
        <XStack items="center" gap="$3">
          <Icon as={Type} size={18} color="$color11" />
          <Text flex={1}>Text size</Text>
        </XStack>
      </ListItem>
    </InsetGroup>
  );
}

function ChatText({ menu, onOpenTextSettings }: Pick<Props, 'menu' | 'onOpenTextSettings'>) {
  const { ChatSizeControl, ChatSpacingControl } = useSkin();
  return (
    <YStack gap="$3" pb="$4">
      <XStack items="center" px="$4">
        <Text variant="large" role="heading" flex={1}>
          Chat text
        </Text>
        <Button variant="ghost" glass={false} size="$3" pressLog="off" onPress={menu.close} testID="skin-session-text-done">
          Done
        </Button>
      </XStack>
      <InsetGroup testID="skin-session-text">
        <ChatSizeControl label="Size" />
        <ChatSpacingControl />
        {onOpenTextSettings ? (
          <ListItem
            variant="grouped"
            showChevron
            pressLog="off"
            accessibilityLabel="More text settings"
            onPress={() => {
              menu.close();
              onOpenTextSettings();
            }}
            testID="skin-session-text-more"
          >
            <Text>More text settings</Text>
          </ListItem>
        ) : null}
      </InsetGroup>
    </YStack>
  );
}

function FileExplorer({ menu, files, workspace }: Pick<Props, 'menu' | 'files' | 'workspace'>) {
  const { path, setPath, back, backLabel } = menu;
  return (
    <YStack flex={1} gap="$2">
      <XStack items="center" gap="$2">
        <IconButton icon={ChevronLeft} variant="secondary" glass={false} size="$3" accessibilityLabel={backLabel} pressLog="off" onPress={back} testID="skin-session-files-back" />
        <YStack flex={1} items="center">
          <Text variant="strong" numberOfLines={1}>
            {path ? fileName(path) : 'Files'}
          </Text>
          <Text variant="muted" numberOfLines={1}>
            {path || workspace}
          </Text>
        </YStack>
        <XStack width={36} />
      </XStack>
      {files ? (
        <>
          {/* the tree stays mounted under an open file, so its folders are still open on the way back */}
          <YStack flex={1} display={path ? 'none' : 'flex'}>
            <FileTree source={files} onRead={setPath} />
          </YStack>
          {path ? <FilePreview source={files} path={path} /> : null}
        </>
      ) : (
        <Centred>
          <Text variant="muted">Waiting for connection…</Text>
        </Centred>
      )}
    </YStack>
  );
}

/** What the header's "more" button opens, as one sheet that changes height with its page: the list, the workspace's files, the chat text. */
export function SessionMenuSheet({ menu, files, workspace, onOpenTextSettings }: Props) {
  const { page } = menu;
  if (!page) return null;
  return (
    <ScreenSheet onClose={menu.close} snapPoints={SNAP_POINTS[page]} handleLabel="Dismiss menu" testID="skin-session-menu">
      {page === 'menu' ? <MenuList menu={menu} /> : null}
      {page === 'text' ? <ChatText menu={menu} onOpenTextSettings={onOpenTextSettings} /> : null}
      {page === 'files' ? <FileExplorer menu={menu} files={files} workspace={workspace} /> : null}
    </ScreenSheet>
  );
}
