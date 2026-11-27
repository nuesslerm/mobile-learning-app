import { useState } from 'react';
import { Platform, Pressable, StyleSheet, Text, TextInput, View } from 'react-native';
import {
  Asterisk,
  ChevronRight,
  FileText,
  Globe,
  Hexagon,
  SquareTerminal,
  X,
} from 'lucide-react-native';
import type { NewTabSheetProps } from '@lib/skin';
import { BottomSheet } from './BottomSheet';
import { homePalettes, useOrcaScheme } from './appearance';

const actions = [
  { kind: 'terminal', title: 'Terminal', icon: SquareTerminal },
  { kind: 'browser', title: 'Browser', icon: Globe },
  { kind: 'markdown', title: 'Markdown Note', icon: FileText },
] as const;

export function NewTabSheet({
  workspace,
  bottomInset,
  agents,
  onAgent,
  onCreate,
  onClose,
}: NewTabSheetProps) {
  const scheme = useOrcaScheme();
  const c = homePalettes[scheme];
  const [browser, setBrowser] = useState(false);
  const [focused, setFocused] = useState(false);
  const [url, setUrl] = useState('');
  const ink = { color: c.textPrimary };
  const sheetColor = scheme === 'light' ? '#f5f5f5' : c.bgPanel;
  const card = { backgroundColor: c.bgGroup };
  return (
    <BottomSheet
      onClose={onClose}
      avoidKeyboard
      style={[
        styles.sheet,
        {
          backgroundColor: sheetColor,
          paddingBottom: Math.max(16, bottomInset),
        },
      ]}
    >
      {(dismiss) => (
        <>
          <View style={[styles.handle, { backgroundColor: c.textMuted }]} />
          {browser ? (
            <>
              {Platform.OS === 'ios' && (
                <View
                  pointerEvents="none"
                  style={[styles.keyboardUnderlay, { backgroundColor: sheetColor }]}
                />
              )}
              <View style={{ padding: 4, paddingVertical: 8 }}>
                <Text
                  accessibilityRole="header"
                  style={[ink, { fontSize: 15, fontWeight: '600' }]}
                >
                  New Browser
                </Text>
                <Text
                  style={{ color: c.textMuted, fontSize: 13, marginTop: 2 }}
                >
                  Enter a URL, or leave blank for a new tab.
                </Text>
              </View>
              <TextInput
                accessibilityLabel="Browser URL"
                autoFocus
                autoCapitalize="none"
                autoCorrect={false}
                keyboardType="url"
                returnKeyType="done"
                selectTextOnFocus
                placeholder="https://example.com"
                placeholderTextColor={c.textMuted}
                value={url}
                onChangeText={setUrl}
                onFocus={() => setFocused(true)}
                onBlur={() => setFocused(false)}
                onSubmitEditing={() =>
                  dismiss(() => onCreate('browser', url.trim() || undefined))
                }
                style={[
                  styles.address,
                  ink,
                  card,
                  {
                    borderWidth: 2,
                    borderColor: focused ? c.accentBlue : c.borderSubtle,
                  },
                ]}
              />
              <View
                style={{
                  flexDirection: 'row',
                  justifyContent: 'flex-end',
                  gap: 8,
                  marginTop: 12,
                }}
              >
                <Pressable
                  accessibilityRole="button"
                  style={styles.open}
                  onPress={() => dismiss()}
                >
                  <Text style={[ink, { fontWeight: '600' }]}>Cancel</Text>
                </Pressable>
                <Pressable
                  accessibilityRole="button"
                  style={[styles.open, { backgroundColor: c.textPrimary }]}
                  onPress={() =>
                    dismiss(() => onCreate('browser', url.trim() || undefined))
                  }
                >
                  <Text style={{ color: c.bgBase, fontWeight: '600' }}>
                    Open
                  </Text>
                </Pressable>
              </View>
            </>
          ) : (
            <>
              <View style={styles.heading}>
                <Text accessibilityRole="header" style={[styles.title, ink]}>
                  New Tab
                </Text>
                <Text
                  numberOfLines={1}
                  style={{ color: c.textMuted, fontSize: 12 }}
                >
                  {workspace}
                </Text>
                <Pressable
                  style={styles.close}
                  accessibilityRole="button"
                  accessibilityLabel="Close"
                  onPress={() => dismiss()}
                >
                  <X size={18} color={c.textPrimary} />
                </Pressable>
              </View>
              {agents.length > 0 && (
                <>
                  <Text style={[styles.label, { color: c.textSecondary }]}>
                    Agents
                  </Text>
                  <View style={styles.agents}>
                    {agents.map((agent) => {
                      const Icon =
                        agent.id === 'claudeAgent'
                          ? Asterisk
                          : agent.id === 'codex'
                            ? Hexagon
                            : SquareTerminal;
                      return (
                        <Pressable
                          accessibilityRole="button"
                          key={agent.id}
                          style={[styles.tile, card]}
                          onPress={() => dismiss(() => onAgent(agent.id))}
                        >
                          <Icon
                            size={21}
                            color={
                              agent.id === 'claudeAgent'
                                ? '#c46e50'
                                : c.textPrimary
                            }
                          />
                          <Text style={ink}>{agent.name}</Text>
                        </Pressable>
                      );
                    })}
                  </View>
                </>
              )}
              <View style={[styles.group, card]}>
                {actions.map(({ kind, title, icon: Icon }, index) => (
                  <Pressable
                    key={kind}
                    accessibilityRole="button"
                    style={[
                      styles.row,
                      index > 0 && {
                        borderTopWidth: StyleSheet.hairlineWidth,
                        borderColor: c.borderSubtle,
                      },
                    ]}
                    onPress={() =>
                      kind === 'browser'
                        ? setBrowser(true)
                        : dismiss(() => onCreate(kind))
                    }
                  >
                    <Icon size={17} color={c.textSecondary} />
                    <Text style={[styles.rowText, ink]}>{title}</Text>
                    {kind === 'browser' && (
                      <ChevronRight size={16} color={c.textMuted} />
                    )}
                  </Pressable>
                ))}
              </View>
            </>
          )}
        </>
      )}
    </BottomSheet>
  );
}
const styles = StyleSheet.create({
  sheet: {
    borderTopLeftRadius: 22,
    borderTopRightRadius: 22,
    paddingHorizontal: 12,
  },
  // Continue the panel behind the rounded top corners of the iOS keyboard.
  keyboardUnderlay: { position: 'absolute', left: 0, right: 0, bottom: -32, height: 32 },
  handle: {
    width: 36,
    height: 4,
    borderRadius: 4,
    alignSelf: 'center',
    marginTop: 8,
    opacity: 0.5,
  },
  heading: {
    alignItems: 'center',
    justifyContent: 'center',
    minHeight: 61,
    gap: 2,
  },
  title: { fontSize: 17, fontWeight: '600' },
  close: { position: 'absolute', right: 0, padding: 14 },
  label: {
    fontSize: 12,
    fontWeight: '500',
    marginHorizontal: 12,
    marginVertical: 7,
  },
  agents: { flexDirection: 'row', gap: 7 },
  tile: {
    flex: 1,
    height: 75,
    borderRadius: 13,
    alignItems: 'center',
    justifyContent: 'center',
    gap: 5,
  },
  group: {
    borderRadius: 14,
    overflow: 'hidden',
    marginTop: 12,
    marginBottom: 10,
  },
  row: {
    height: 46,
    alignItems: 'center',
    flexDirection: 'row',
    gap: 12,
    paddingHorizontal: 14,
  },
  rowText: { flex: 1, fontSize: 15 },
  address: { borderRadius: 12, paddingHorizontal: 12, height: 46 },
  open: {
    alignSelf: 'flex-end',
    borderRadius: 12,
    paddingHorizontal: 18,
    paddingVertical: 10,
  },
});
