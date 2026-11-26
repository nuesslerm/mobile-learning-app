import { BottomSheet } from './BottomSheet';
import { ChatSizeControl, ChatSpacingControl } from './ChatTextControls';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import type { SessionHeaderProps } from '@lib/agent-ui-contracts';
import {
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Ellipsis,
  FileText,
  Type,
  Folder,
  FolderOpen,
} from 'lucide-react-native';
import { useCallback, useEffect, useRef, useState } from 'react';
import {
  ActivityIndicator,
  FlatList,
  Modal,
  Platform,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  View,
} from 'react-native';
import { homePalettes, useOrcaScheme } from './appearance';

type Source = NonNullable<SessionHeaderProps['files']>;
type Listing = Awaited<ReturnType<Source['listDirectory']>>;
type Entry = Listing['entries'][number];
type FolderState = { result?: Listing; failed?: boolean };

function FileTree({
  source,
  onRead,
}: {
  source: Source;
  onRead: (path: string) => void;
}) {
  const colors = homePalettes[useOrcaScheme()];
  const [folders, setFolders] = useState<Record<string, FolderState>>({});
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const generation = useRef(0);
  const requested = useRef(new Set<string>());
  const load = useCallback(
    (path: string) => {
      if (requested.current.has(path)) return;
      requested.current.add(path);
      const active = generation.current;
      setFolders((prev) => ({ ...prev, [path]: {} }));
      void source.listDirectory(path).then(
        (result) => {
          if (active === generation.current)
            setFolders((prev) => ({ ...prev, [path]: { result } }));
        },
        () => {
          if (active !== generation.current) return;
          requested.current.delete(path);
          setFolders((prev) => ({ ...prev, [path]: { failed: true } }));
        }
      );
    },
    [source]
  );
  useEffect(() => {
    requested.current.clear();
    setExpanded(new Set());
    setFolders({});
    load('');
    return () => {
      generation.current++;
    };
  }, [load]);

  type Row =
    | { entry: Entry; depth: number }
    | { status: string; parent: string; depth: number; retry?: boolean };
  const rows: Row[] = [];
  function append(path: string, depth: number) {
    const folder = folders[path];
    if (!folder?.result) {
      rows.push({
        status: folder?.failed
          ? 'Could not load files. Try again'
          : 'Loading files…',
        parent: path,
        depth,
        retry: folder?.failed,
      });
      return;
    }
    const entries = folder.result.entries
      .filter((entry) => {
        const name = entry.path.split('/').pop();
        return name !== '.git' && name !== 'node_modules';
      })
      .sort((a, b) =>
        a.kind === b.kind
          ? a.path.localeCompare(b.path, undefined, {
              numeric: true,
              sensitivity: 'base',
            })
          : a.kind === 'directory'
            ? -1
            : 1
      );
    for (const entry of entries) {
      rows.push({ entry, depth });
      if (entry.kind === 'directory' && expanded.has(entry.path))
        append(entry.path, depth + 1);
    }
    if (folder.result.truncated || entries.length === 0)
      rows.push({
        status: folder.result.truncated
          ? 'Listing truncated'
          : 'No files found',
        parent: path,
        depth,
      });
  }
  append('', 0);
  function open(entry: Entry) {
    if (entry.kind === 'file') {
      onRead(entry.path);
      return;
    }
    const next = new Set(expanded);
    if (next.has(entry.path)) next.delete(entry.path);
    else {
      next.add(entry.path);
      load(entry.path);
    }
    setExpanded(next);
  }
  if (!folders['']?.result && !folders['']?.failed)
    return (
      <View style={styles.center}>
        <ActivityIndicator color={colors.textSecondary} />
        <Text style={{ color: colors.textSecondary }}>Loading files…</Text>
      </View>
    );
  return (
    <FlatList
      data={rows}
      contentContainerStyle={{ paddingVertical: 8 }}
      keyExtractor={(row) =>
        'entry' in row ? row.entry.path : 'status:' + row.parent
      }
      renderItem={({ item }) => {
        const paddingLeft = 12 + item.depth * 16;
        if ('status' in item)
          return (
            <Pressable
              disabled={!item.retry}
              accessibilityRole={item.retry ? 'button' : undefined}
              onPress={() => load(item.parent)}
              style={[styles.row, { paddingLeft }]}
            >
              <Text style={{ color: colors.textSecondary }}>{item.status}</Text>
            </Pressable>
          );
        const { entry } = item;
        const name = entry.path.split('/').pop() ?? entry.path;
        const directory = entry.kind === 'directory';
        const isOpen = expanded.has(entry.path);
        const Arrow = isOpen ? ChevronDown : ChevronRight;
        const Icon = directory ? (isOpen ? FolderOpen : Folder) : FileText;
        return (
          <Pressable
            accessibilityRole="button"
            accessibilityLabel={name}
            accessibilityState={directory ? { expanded: isOpen } : {}}
            onPress={() => open(entry)}
            style={({ pressed }) => [
              styles.row,
              {
                paddingLeft,
                backgroundColor: pressed ? colors.bgRaised : undefined,
              },
            ]}
          >
            {directory ? (
              <Arrow size={16} color={colors.textSecondary} />
            ) : (
              <View style={{ width: 16 }} />
            )}
            <Icon size={17} color={colors.textSecondary} />
            <Text
              numberOfLines={1}
              style={{ flex: 1, fontSize: 15, color: colors.textPrimary }}
            >
              {name}
            </Text>
          </Pressable>
        );
      }}
    />
  );
}

function Preview({ source, path }: { source: Source; path: string }) {
  const colors = homePalettes[useOrcaScheme()];
  const [file, setFile] = useState<Awaited<ReturnType<Source['readFile']>>>();
  const [failed, setFailed] = useState(false);
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let active = true;
    setFile(undefined);
    setFailed(false);
    void source.readFile(path).then(
      (value) => {
        if (active) setFile(value);
      },
      () => {
        if (active) setFailed(true);
      }
    );
    return () => {
      active = false;
    };
  }, [source, path, attempt]);
  if (failed)
    return (
      <Pressable
        accessibilityRole="button"
        style={styles.center}
        onPress={() => setAttempt((value) => value + 1)}
      >
        <Text style={{ color: colors.textPrimary }}>
          Could not read this file. Try again
        </Text>
      </Pressable>
    );
  if (!file)
    return (
      <View style={styles.center}>
        <ActivityIndicator color={colors.textSecondary} />
      </View>
    );
  return (
    <ScrollView contentContainerStyle={{ padding: 12 }}>
      {file.truncated && (
        <Text style={{ color: colors.textSecondary }}>
          Showing the first 1 MiB
        </Text>
      )}
      <Text
        selectable
        style={{
          color: colors.textPrimary,
          fontSize: 13,
          fontFamily: Platform.OS === 'ios' ? 'Menlo' : 'monospace',
        }}
      >
        {file.contents}
      </Text>
    </ScrollView>
  );
}

export function SessionFiles({
  files,
  workspace,
  onOpenTextSettings,
  topInset,
}: Pick<
  SessionHeaderProps,
  'files' | 'workspace' | 'topInset' | 'onOpenTextSettings'
>) {
  const colors = homePalettes[useOrcaScheme()];
  const [page, setPage] = useState<'menu' | 'files' | 'text'>();
  const { bottom } = useSafeAreaInsets();
  const [path, setPath] = useState('');
  function back() {
    if (path) setPath('');
    else setPage(undefined);
  }
  return (
    <>
      <Pressable
        onPress={() => setPage('menu')}
        accessibilityLabel="More actions"
        accessibilityRole="button"
        style={[
          styles.circle,
          styles.more,
          { backgroundColor: colors.bgRaised },
        ]}
      >
        <Ellipsis size={18} color={colors.textPrimary} />
      </Pressable>
      {page === 'text' && (
        <BottomSheet
          label="Close Chat text"
          onClose={back}
          style={{
            paddingHorizontal: 12,
            paddingBottom: bottom + 16,
            paddingTop: 8,
            borderTopLeftRadius: 22,
            borderTopRightRadius: 22,
            backgroundColor: colors.bgRaised,
          }}
        >
          {(dismiss) => (
            <>
              <Pressable
                onPress={() => dismiss()}
                accessibilityLabel="Dismiss Chat text"
                accessibilityRole="button"
                style={{
                  alignSelf: 'center',
                  width: 44,
                  height: 16,
                  alignItems: 'center',
                }}
              >
                <View
                  style={{
                    width: 36,
                    height: 5,
                    borderRadius: 3,
                    backgroundColor: colors.textMuted,
                  }}
                />
              </Pressable>
              <View
                style={{
                  height: 48,
                  alignItems: 'center',
                  justifyContent: 'center',
                }}
              >
                <Text
                  accessibilityRole="header"
                  style={{
                    fontSize: 16,
                    fontWeight: '600',
                    color: colors.textPrimary,
                  }}
                >
                  Chat text
                </Text>
                <Pressable
                  accessibilityRole="button"
                  onPress={() => dismiss()}
                  style={{ position: 'absolute', right: 8, padding: 8 }}
                >
                  <Text
                    style={{
                      fontSize: 16,
                      fontWeight: '600',
                      color: colors.textPrimary,
                    }}
                  >
                    Done
                  </Text>
                </Pressable>
              </View>
              <View
                style={{
                  borderRadius: 16,
                  backgroundColor: colors.bgGroup,
                  overflow: 'hidden',
                }}
              >
                <ChatSizeControl label="Size" />
                <ChatSpacingControl />
                {onOpenTextSettings && (
                  <Pressable
                    accessibilityRole="button"
                    onPress={() =>
                      dismiss(() => {
                        setPage(undefined);
                        onOpenTextSettings();
                      })
                    }
                    style={[styles.menuItem, { height: 48 }]}
                  >
                    <Text style={{ fontSize: 15, color: colors.textPrimary }}>
                      More text settings
                    </Text>
                    <ChevronRight size={16} color={colors.textSecondary} />
                  </Pressable>
                )}
              </View>
            </>
          )}
        </BottomSheet>
      )}
      {page !== undefined && page !== 'text' && (
        <Modal visible transparent animationType="none" onRequestClose={back}>
          {page === 'menu' ? (
            <>
              <Pressable
                accessibilityRole="button"
                accessibilityLabel="Dismiss menu"
                onPress={back}
                style={StyleSheet.absoluteFill}
              />
              <View
                style={[
                  styles.menu,
                  {
                    top: topInset + 50,
                    backgroundColor: colors.bgPanel,
                    borderColor: colors.borderSubtle,
                  },
                ]}
              >
                <Pressable
                  accessibilityRole="button"
                  onPress={() => setPage('files')}
                  accessibilityLabel="File explorer"
                  style={styles.menuItem}
                >
                  <Text style={{ fontSize: 15, color: colors.textPrimary }}>
                    File explorer
                  </Text>
                  <Folder size={17} color={colors.textPrimary} />
                </Pressable>
                <Pressable
                  accessibilityRole="button"
                  accessibilityLabel="Text size"
                  style={[
                    styles.menuItem,
                    {
                      borderTopWidth: StyleSheet.hairlineWidth,
                      borderColor: colors.borderSubtle,
                    },
                  ]}
                  onPress={() => setPage('text')}
                >
                  <Text style={{ fontSize: 15, color: colors.textPrimary }}>
                    Text size
                  </Text>
                  <Type size={17} color={colors.textPrimary} />
                </Pressable>
              </View>
            </>
          ) : page === 'files' ? (
            <View
              style={{
                flex: 1,
                paddingTop: topInset,
                backgroundColor: colors.bgBase,
              }}
            >
              <View style={[styles.bar, { borderColor: colors.borderSubtle }]}>
                <View style={styles.titles}>
                  <Text
                    numberOfLines={1}
                    style={{
                      color: colors.textPrimary,
                      fontSize: 16,
                      fontWeight: '600',
                    }}
                  >
                    {path ? path.split('/').pop() : 'Files'}
                  </Text>
                  <Text
                    numberOfLines={1}
                    style={{ color: colors.textSecondary, fontSize: 12 }}
                  >
                    {path || workspace}
                  </Text>
                </View>
                <Pressable
                  accessibilityRole="button"
                  accessibilityLabel={path ? 'Back to files' : 'Back to chat'}
                  onPress={back}
                  style={[styles.circle, { backgroundColor: colors.bgRaised }]}
                >
                  <ChevronLeft size={18} color={colors.textPrimary} />
                </Pressable>
              </View>
              {files ? (
                <>
                  <View style={{ flex: 1, display: path ? 'none' : 'flex' }}>
                    <FileTree source={files} onRead={setPath} />
                  </View>
                  {path ? <Preview source={files} path={path} /> : null}
                </>
              ) : (
                <View style={styles.center}>
                  <Text style={{ color: colors.textSecondary }}>
                    Waiting for connection…
                  </Text>
                </View>
              )}
            </View>
          ) : null}
        </Modal>
      )}
    </>
  );
}

const styles = StyleSheet.create({
  more: { position: 'absolute', right: 12, alignSelf: 'center' },
  circle: {
    height: 36,
    width: 36,
    borderRadius: 18,
    justifyContent: 'center',
    alignItems: 'center',
  },
  menu: {
    position: 'absolute',
    right: 12,
    width: 252,
    borderRadius: 16,
    borderWidth: StyleSheet.hairlineWidth,
    shadowColor: '#000',
    shadowOffset: { width: 0, height: 12 },
    shadowOpacity: 0.24,
    shadowRadius: 12,
    elevation: 8,
  },
  menuItem: {
    height: 44,
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    paddingHorizontal: 16,
  },
  bar: {
    height: 52,
    paddingHorizontal: 12,
    justifyContent: 'center',
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  titles: {
    ...StyleSheet.absoluteFill,
    alignItems: 'center',
    justifyContent: 'center',
    paddingHorizontal: 58,
  },
  center: { flex: 1, justifyContent: 'center', alignItems: 'center', gap: 8 },
  row: {
    minHeight: 44,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    paddingRight: 12,
  },
});
