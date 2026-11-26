import { ChevronLeft, SquareTerminal } from 'lucide-react-native';
import { Pressable, StyleSheet, Text, View } from 'react-native';
import type { SessionHeaderProps } from '@lib/agent-ui-contracts';
import { SessionFiles } from './SessionFiles';
import { homePalettes, useOrcaScheme } from './appearance';

export function SessionHeader(props: SessionHeaderProps) {
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
        <View style={styles.center} pointerEvents="box-none">
          <Text numberOfLines={1} style={[styles.workspace, ink]}>
            {props.workspace}
          </Text>
          <Pressable
            disabled={props.connected}
            onPress={props.onReconnect}
            accessibilityRole={props.connected ? undefined : 'button'}
            accessibilityLabel={
              props.connected ? undefined : 'Reconnect to desktop'
            }
            style={styles.meta}
          >
            <View
              style={[
                styles.dot,
                {
                  backgroundColor: props.connected
                    ? colors.statusGreen
                    : colors.statusAmber,
                },
              ]}
            />
            <Text style={{ fontSize: 12, color: colors.textSecondary }}>
              {props.connected ? 'Connected' : 'Connecting…'}
            </Text>
          </Pressable>
        </View>
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
        <SessionFiles files={props.files} workspace={props.workspace} topInset={props.topInset}/>
      </View>
      <View style={[styles.tabs, border]}>
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
          <View style={[styles.countBox, { borderColor: colors.textPrimary }]}>
            <Text style={[styles.count, ink]}>{props.tabCount}</Text>
          </View>
        </Pressable>
      </View>
    </View>
  );
}

const styles = StyleSheet.create({
  chrome: { borderBottomWidth: 1 },
  header: { height: 52, paddingHorizontal: 12, justifyContent: 'center' },
  center: {
    ...StyleSheet.absoluteFill,
    alignItems: 'center',
    justifyContent: 'center',
    paddingHorizontal: 64,
  },
  workspace: { fontSize: 16, fontWeight: '600' },
  meta: { flexDirection: 'row', alignItems: 'center', gap: 8, marginTop: 2 },
  dot: { width: 8, height: 8, borderRadius: 4 },
  back: {
    width: 36,
    height: 36,
    alignItems: 'center',
    justifyContent: 'center',
    borderRadius: 18,
  },
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
