import type { HostWorkspaceListProps } from '@lib/agent-ui-contracts';
import Bell from 'lucide-react-native/dist/esm/icons/bell.mjs';
import Pin from 'lucide-react-native/dist/esm/icons/pin.mjs';
import Folder from 'lucide-react-native/dist/esm/icons/folder.mjs';
import ChevronDown from 'lucide-react-native/dist/esm/icons/chevron-down.mjs';
import {
  Platform,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from 'react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { BackIcon, ChevronRightIcon, SearchIcon } from './homeIcons';
import { useChatPalette } from './chatPalette';

export function HostWorkspaceList(props: HostWorkspaceListProps) {
  const c = useChatPalette();
  const folded = (key: string) => props.disclosure?.keys.has(key) ?? false;
  const ink = { color: c.textPrimary };
  const muted = { color: c.textSecondary };
  return (
    <SafeAreaView style={[styles.screen, { backgroundColor: c.bgBase }]}>
      <View style={styles.nav}>
        <Pressable
          accessibilityRole="button"
          accessibilityLabel="Back"
          onPress={props.onBack}
          style={[styles.back, { backgroundColor: c.bgRaised }]}
        >
          <BackIcon size={20} color={c.textPrimary} />
        </Pressable>
      </View>
      <ScrollView
        keyboardShouldPersistTaps="handled"
        contentContainerStyle={styles.content}
      >
        <Text accessibilityRole="header" style={[styles.title, ink]}>
          {props.title}
        </Text>
        {props.platform && (
          <View style={styles.platform}>
            <View style={[styles.dot, { backgroundColor: c.statusGreen }]} />
            <Text style={[styles.metadata, muted]}>{props.platform}</Text>
          </View>
        )}
        {props.sections.map(({ key, label, rows, icon }) => (
          <View key={key}>
            {label && (
              <Pressable
                accessibilityRole="button"
                accessibilityLabel={label + ' ' + rows.length}
                aria-expanded={!folded(key)}
                accessibilityState={{ expanded: !folded(key), disabled: !props.disclosure?.ready }}
                disabled={!props.disclosure?.ready}
                style={styles.group}
                onPress={() => props.disclosure?.change(key, !folded(key))}
              >
                <View style={styles.disclosure}>
                  {folded(key) ? (
                    <ChevronRightIcon size={12} color={c.textMuted} />
                  ) : (
                    <ChevronDown size={12} color={c.textMuted} />
                  )}
                </View>
                {icon === 'pin' && <Pin size={12} color={c.textMuted} />}
                {icon === 'folder' && <Folder size={15} color={c.textMuted} />}
                <Text style={[styles.groupText, muted, icon && { marginLeft: 4 }]}>
                  {label}
                </Text>
                <Text style={[styles.count, { color: c.textMuted }]}>{rows.length}</Text>
              </Pressable>
            )}
            {(!label || !folded(key)) &&
              rows.map((row) => (
                <Pressable
                  key={row.id}
                  accessibilityLabel={row.name}
                  accessibilityRole="button"
                  onPress={() => props.onSelect(row.id)}
                  onLongPress={
                    props.onActions && (() => props.onActions?.(row.id))
                  }
                  accessibilityActions={
                    props.onActions
                      ? [{ name: 'longpress', label: 'Workspace actions' }]
                      : undefined
                  }
                  onAccessibilityAction={(event) => {
                    if (event.nativeEvent.actionName === 'longpress')
                      props.onActions?.(row.id);
                  }}
                  {...(Platform.OS === 'web' && props.onActions
                    ? {
                        onContextMenu: (event: {
                          preventDefault: () => void;
                        }) => {
                          event.preventDefault();
                          props.onActions?.(row.id);
                        },
                      }
                    : {})}
                  style={styles.row}
                >
                  <View style={styles.indicators}>
                    <View
                      style={[
                        styles.dot,
                        styles.rowDot,
                        { backgroundColor: c.textMuted },
                      ]}
                    />
                    {row.unread && (
                      <Bell
                        size={10}
                        color={c.statusAmber}
                        fill={c.statusAmber}
                        style={styles.bell}
                      />
                    )}
                  </View>
                  <View style={[styles.body, { borderColor: c.borderSubtle }]}>
                    <View style={styles.nameRow}>
                      {row.projectInTitle && <Folder size={11} color={c.textMuted} />}
                      <Text numberOfLines={1} style={[styles.name, ink]}>{row.name}</Text>
                    </View>
                    {row.project ? (
                      <View style={styles.projectDetail}>
                        <Folder size={11} color={c.textMuted} />
                        <Text numberOfLines={1} style={[styles.project, muted]}>{row.project}</Text>
                        {row.detail && <Text numberOfLines={1} style={[styles.projectBranch, { color: c.textMuted }]}>{row.detail}</Text>}
                      </View>
                    ) : row.detail && (
                      <Text numberOfLines={1} style={[styles.branch, muted]}>
                        {row.detail}
                      </Text>
                    )}
                  </View>
                </Pressable>
              ))}
          </View>
        ))}
        {props.notice && (
          <Text accessibilityLiveRegion="polite" style={[styles.notice, muted]}>
            {props.notice}
          </Text>
        )}
        {props.retry && (
          <Pressable accessibilityRole="button" onPress={props.retry}>
            <Text style={[styles.notice, ink]}>Try again</Text>
          </Pressable>
        )}
      </ScrollView>
      {props.search && (
        <View style={styles.toolbar}>
          {props.options}
          <View
            style={[
              styles.search,
              { backgroundColor: c.bgGroup, borderColor: c.borderSubtle },
            ]}
          >
            <SearchIcon size={16} color={c.textMuted} />
            <TextInput
              accessibilityLabel="Search workspaces"
              placeholder="Search workspaces"
              placeholderTextColor={c.textMuted}
              value={props.search.value}
              onChangeText={props.search.onChange}
              autoCapitalize="none"
              autoCorrect={false}
              style={[styles.input, ink]}
            />
          </View>
        </View>
      )}
    </SafeAreaView>
  );
}

const styles = StyleSheet.create({
  screen: { flex: 1 },
  nav: { paddingHorizontal: 12, height: 54, justifyContent: 'center' },
  back: {
    width: 36,
    height: 36,
    borderRadius: 18,
    alignItems: 'center',
    justifyContent: 'center',
  },
  content: { paddingBottom: 16 },
  title: {
    marginHorizontal: 16,
    fontSize: 24,
    fontWeight: '700',
    letterSpacing: -0.9,
    marginTop: 4,
  },
  platform: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 5,
    marginHorizontal: 16,
    marginTop: 4,
    marginBottom: 16,
  },
  dot: { width: 8, height: 8, borderRadius: 4 },
  metadata: { fontSize: 12 },
  group: {
    marginHorizontal: 18,
    height: 40,
    flexDirection: 'row',
    alignItems: 'center',
  },
  disclosure: { width: 20, alignItems: 'center', marginRight: 8 },
  groupText: { fontSize: 13, fontWeight: '500' },
  count: { fontSize: 13, marginLeft: 6 },
  projectDetail: { flexDirection: 'row', alignItems: 'center', gap: 4, marginTop: 2 },
  project: { fontSize: 11, maxWidth: 100 },
  projectBranch: { fontSize: 11, fontFamily: 'monospace', flex: 1 },
  row: { flexDirection: 'row', alignItems: 'flex-start', paddingLeft: 24 },
  indicators: { marginTop: 19, width: 8, alignItems: 'center' },
  rowDot: { opacity: 0.4 },
  bell: { marginTop: 6, marginBottom: 12 },
  body: {
    flex: 1,
    marginLeft: 14,
    paddingVertical: 12,
    minHeight: 44,
    justifyContent: 'center',
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  nameRow: { flexDirection: 'row', alignItems: 'center', gap: 8 },
  name: { flexShrink: 1, fontSize: 14, fontWeight: '600' },
  branch: { fontFamily: 'monospace', fontSize: 11, marginTop: 3 },
  notice: { margin: 16, fontSize: 15 },
  toolbar: { flexDirection: 'row', alignItems: 'center', paddingRight: 14 },
  search: {
    flex: 1,
    height: 44,
    borderWidth: 1,
    borderRadius: 24,
    paddingHorizontal: 12,
    flexDirection: 'row',
    gap: 8,
    alignItems: 'center',
  },
  input: { flex: 1, fontSize: 16 },
});
