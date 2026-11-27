import { useEffect, useRef, type ComponentRef } from 'react';
import {
  ChevronLeft,
  GitBranch,
  Globe,
  FileText,
  MessageSquare,
  Plus,
  SquareTerminal,
} from 'lucide-react-native';
import { Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';
import type { SessionHeaderProps } from '@lib/skin';
import { SessionWorkspaceTitle } from './SessionWorkspaceTitle';
import { SessionFiles } from './SessionFiles';
import { homePalettes, useOrcaScheme } from './appearance';

export function SessionHeader(props: SessionHeaderProps) {
  const strip = useRef<ComponentRef<typeof ScrollView>>(null);
  const positions = useRef(new Map<string, number>());
  const reveal = () => {
    const x = props.activeId
      ? positions.current.get(props.activeId)
      : undefined;
    if (x !== undefined)
      strip.current?.scrollTo({ x: Math.max(0, x - 12), animated: false });
  };
  useEffect(reveal, [props.activeId]);
  const colors = homePalettes[useOrcaScheme()];
  const ink = { color: colors.textPrimary };
  const border = { borderColor: colors.borderSubtle };
  return (
    <View
      testID="MobileT3Header"
      style={[
        styles.chrome,
        border,
        { paddingTop: props.topInset, backgroundColor: colors.bgPanel },
      ]}
    >
      <View style={styles.header}>
        <SessionWorkspaceTitle {...props} />
        <Pressable
          onPress={props.onBack}
          accessibilityLabel="Back to worktrees"
          accessibilityRole="button"
          hitSlop={4}
          style={({ pressed }) => [
            styles.back,
            { backgroundColor: colors.bgRaised, opacity: pressed ? 0.7 : 1 },
          ]}
        >
          <ChevronLeft size={18} strokeWidth={2.1} color={colors.textPrimary} />
        </Pressable>
        {props.onOpenSourceControl && (
          <Pressable
            accessibilityRole="button"
            accessibilityLabel="Open source control"
            onPress={props.onOpenSourceControl}
            hitSlop={4}
            style={({ pressed }) => [
              styles.back,
              styles.source,
              { backgroundColor: colors.bgRaised, opacity: pressed ? 0.7 : 1 },
            ]}
          >
            <GitBranch size={18} strokeWidth={2.1} color={colors.textPrimary} />
          </Pressable>
        )}
        <SessionFiles
          onOpenTextSettings={props.onOpenTextSettings}
          files={props.files}
          workspace={props.workspace}
          topInset={props.topInset}
        />
      </View>
      <View style={[styles.tabs, border]}>
        {props.tabs ? (
          <ScrollView
            ref={strip}
            onContentSizeChange={reveal}
            horizontal
            showsHorizontalScrollIndicator={false}
            style={{ flex: 1 }}
            keyboardShouldPersistTaps="always"
          >
            {props.tabs.map((tab) => {
              const selected = tab.id === props.activeId;
              const Icon =
                tab.kind === 'browser'
                  ? Globe
                  : tab.kind === 'markdown'
                    ? FileText
                    : tab.kind === 'terminal'
                      ? SquareTerminal
                      : MessageSquare;
              return (
                <Pressable
                  key={tab.id}
                  onLayout={(e) => {
                    positions.current.set(tab.id, e.nativeEvent.layout.x);
                    if (selected) reveal();
                  }}
                  accessibilityRole="tab"
                  accessibilityLabel={tab.title}
                  accessibilityState={{ selected }}
                  onPress={() => props.onSelectTab?.(tab.id)}
                  onLongPress={props.onShowTabs}
                  style={{
                    flexDirection: 'row',
                    alignItems: 'center',
                    paddingHorizontal: 10,
                    gap: 5,
                    maxWidth: 148,
                    borderRightWidth: StyleSheet.hairlineWidth,
                    borderRightColor: colors.borderSubtle,
                    borderBottomWidth: 2,
                    borderBottomColor: selected
                      ? colors.textPrimary
                      : 'transparent',
                    backgroundColor: selected
                      ? colors.bgRaised
                      : colors.bgPanel,
                  }}
                >
                  <Icon size={13} color={colors.textSecondary} />
                  <Text
                    numberOfLines={1}
                    style={{
                      fontSize: 13,
                      flexShrink: 1,
                      color: selected
                        ? colors.textPrimary
                        : colors.textSecondary,
                    }}
                  >
                    {tab.title}
                  </Text>
                </Pressable>
              );
            })}
          </ScrollView>
        ) : (
          <>
            <View
              style={[
                styles.activeTab,
                border,
                {
                  backgroundColor: colors.bgRaised,
                  borderBottomColor: colors.textSecondary,
                },
              ]}
            >
              <SquareTerminal
                size={14}
                color={colors.textSecondary}
                strokeWidth={2.1}
              />
              <Text numberOfLines={1} style={[styles.title, ink]}>
                {props.title}
              </Text>
            </View>
            <Pressable
              accessibilityRole="button"
              accessibilityHint="Shows all tabs"
              accessibilityLabel={
                props.tabCount === 1 ? '1 tab' : props.tabCount + ' tabs'
              }
              onPress={props.onShowTabs}
              style={styles.countCell}
            >
              <View
                style={[styles.countBox, { borderColor: colors.textPrimary }]}
              >
                <Text style={[styles.count, ink]}>{props.tabCount}</Text>
              </View>
            </Pressable>
          </>
        )}
        {props.onNewTab && (
          <Pressable
            accessibilityRole="button"
            accessibilityLabel="New Tab"
            style={[styles.newTab, { backgroundColor: colors.bgRaised }]}
            onPress={props.onNewTab}
          >
            <Plus size={18} color={colors.textPrimary} />
          </Pressable>
        )}
      </View>
    </View>
  );
}

const styles = StyleSheet.create({
  chrome: { borderBottomWidth: 1 },
  header: { height: 52, paddingHorizontal: 12, justifyContent: 'center' },
  back: {
    width: 36,
    height: 36,
    alignItems: 'center',
    justifyContent: 'center',
    borderRadius: 18,
  },
  source: { position: 'absolute', right: 56 },
  tabs: { height: 44, flexDirection: 'row', borderTopWidth: 1 },
  activeTab: {
    flex: 1,
    minWidth: 0,
    paddingLeft: 16,
    paddingRight: 8,
    flexDirection: 'row',
    gap: 6,
    alignItems: 'center',
    borderRightWidth: 1,
    borderBottomWidth: 2,
  },
  title: { fontSize: 14, fontWeight: '600', flexShrink: 1 },
  newTab: {
    alignSelf: 'center',
    width: 34,
    height: 34,
    borderRadius: 17,
    marginHorizontal: 6,
    alignItems: 'center',
    justifyContent: 'center',
  },
  countCell: { width: 46, alignItems: 'center', justifyContent: 'center' },
  countBox: {
    minWidth: 19,
    height: 19,
    paddingHorizontal: 3,
    borderRadius: 6,
    borderWidth: 1.8,
    alignItems: 'center',
    justifyContent: 'center',
  },
  count: { fontSize: 11, fontWeight: '700', fontVariant: ['tabular-nums'] },
});
