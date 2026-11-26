import { useState } from 'react';
import { Pressable, StyleSheet, Text, TextInput, View } from 'react-native';
import {
  Asterisk,
  ChevronLeft,
  ChevronRight,
  FileText,
  Globe,
  Hexagon,
  SquareTerminal,
  X,
} from 'lucide-react-native';
import type { NewTabSheetProps } from '@lib/agent-ui-contracts';
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
  const [url, setUrl] = useState('');
  const ink = { color: c.textPrimary };
  const card = { backgroundColor: c.bgGroup };
  return (
    <BottomSheet
      onClose={onClose}
      avoidKeyboard
      style={[
        styles.sheet,
        {
          backgroundColor: scheme === 'light' ? '#f5f5f5' : c.bgPanel,
          paddingBottom: Math.max(16, bottomInset),
        },
      ]}
    >
      {(dismiss) => (
        <>
          <View style={[styles.handle, { backgroundColor: c.textMuted }]} />
          <View style={styles.heading}>
            {browser && (
              <Pressable
                accessibilityLabel="Back"
                accessibilityRole="button"
                style={styles.back}
                onPress={() => setBrowser(false)}
              >
                <ChevronLeft size={19} color={c.textPrimary} />
              </Pressable>
            )}
            <Text accessibilityRole="header" style={[styles.title, ink]}>
              {browser ? 'New Browser Tab' : 'New Tab'}
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
          {browser ? (
            <>
              <TextInput
                accessibilityLabel="Browser URL"
                autoFocus
                autoCapitalize="none"
                autoCorrect={false}
                keyboardType="url"
                placeholder="Enter URL"
                placeholderTextColor={c.textMuted}
                value={url}
                onChangeText={setUrl}
                style={[styles.address, ink, card]}
              />
              <Pressable
                accessibilityRole="button"
                style={[styles.open, card]}
                onPress={() =>
                  dismiss(() => onCreate('browser', url.trim() || undefined))
                }
              >
                <Text style={ink}>Open</Text>
              </Pressable>
            </>
          ) : (
            <>
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
  back: { position: 'absolute', left: 0, padding: 14 },
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
  address: { borderRadius: 12, padding: 14, marginVertical: 8 },
  open: {
    alignSelf: 'flex-end',
    borderRadius: 9,
    paddingHorizontal: 18,
    paddingVertical: 10,
  },
});
