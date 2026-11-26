import type { HostWorkspaceListProps } from '@lib/agent-ui-contracts';
import Folder from 'lucide-react-native/dist/esm/icons/folder.mjs';
import ChevronDown from 'lucide-react-native/dist/esm/icons/chevron-down.mjs';
import { useState } from 'react';
import {
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
  const [collapsed, setCollapsed] = useState(false);
  const ink = { color: c.textPrimary };
  const muted = { color: c.textSecondary };
  const Toggle = collapsed ? ChevronRightIcon : ChevronDown;
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
        {props.group && (
          <Pressable
            accessibilityRole="button"
            accessibilityState={{ expanded: !collapsed }}
            style={styles.group}
            onPress={() => setCollapsed(!collapsed)}
          >
            <Toggle size={13} color={c.textMuted} />
            <Folder size={15} color={c.textMuted} />
            <Text style={[styles.groupText, muted]}>
              {props.group} {props.rows.length}
            </Text>
          </Pressable>
        )}
        {(!props.group || !collapsed) &&
          props.rows.map((row) => (
            <Pressable
              key={row.id}
              accessibilityLabel={row.name}
              accessibilityRole="button"
              onPress={() => props.onSelect(row.id)}
              style={styles.row}
            >
              <View
                style={[
                  styles.dot,
                  styles.rowDot,
                  { backgroundColor: c.textMuted },
                ]}
              />
              <View style={[styles.body, { borderColor: c.borderSubtle }]}>
                <Text numberOfLines={1} style={[styles.name, ink]}>
                  {row.name}
                </Text>
                {row.detail && (
                  <Text numberOfLines={1} style={[styles.branch, muted]}>
                    {row.detail}
                  </Text>
                )}
              </View>
            </Pressable>
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
    marginHorizontal: 22,
    height: 40,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
  },
  groupText: { fontSize: 13, fontWeight: '500' },
  row: { flexDirection: 'row', alignItems: 'flex-start', paddingLeft: 24 },
  rowDot: { marginTop: 19, opacity: 0.4 },
  body: {
    flex: 1,
    marginLeft: 14,
    paddingVertical: 12,
    minHeight: 44,
    justifyContent: 'center',
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  name: { fontSize: 15, fontWeight: '600' },
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
