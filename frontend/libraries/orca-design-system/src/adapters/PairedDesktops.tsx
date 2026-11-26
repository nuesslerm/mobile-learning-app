// Desktop list from Orca mobile 2c0cbbc2 (MIT, Lovecast Inc.).
import type { PairedDesktopsProps } from '@lib/agent-ui-contracts';
import Monitor from 'lucide-react-native/dist/esm/icons/monitor.mjs';
import { Pressable, StyleSheet, Text, View } from 'react-native';
import {
  homePalettes,
  homeRadii,
  homeSpacing,
  useOrcaScheme,
} from './appearance';
import { ChevronRightIcon, homeIcons } from './homeIcons';

export function PairedDesktops({
  desktop,
  onPair,
  onOpen,
}: PairedDesktopsProps) {
  const c = homePalettes[useOrcaScheme()];
  const Plus = homeIcons.add;
  return (
    <>
      <Text
        accessibilityRole="header"
        style={[styles.heading, { color: c.textPrimary }]}
      >
        Desktops
      </Text>
      <View style={[styles.group, { backgroundColor: c.bgPanel }]}>
        {desktop && (
          <>
            <Pressable
              accessibilityRole="button"
              accessibilityLabel={'Open ' + desktop.name}
              accessibilityHint={desktop.summary}
              onPress={onOpen}
              style={({ pressed }) => [
                styles.row,
                pressed && { backgroundColor: c.bgRaised },
              ]}
            >
              <View style={[styles.tile, { backgroundColor: c.bgRaised }]}>
                <Monitor size={17} strokeWidth={1.8} color={c.textPrimary} />
                <View
                  style={[
                    styles.dot,
                    { borderColor: c.bgPanel, backgroundColor: c.statusGreen },
                  ]}
                />
              </View>
              <View style={styles.text}>
                <Text
                  numberOfLines={1}
                  style={[styles.title, { color: c.textPrimary }]}
                >
                  {desktop.name}
                </Text>
                <Text
                  numberOfLines={1}
                  style={[styles.subtitle, { color: c.textSecondary }]}
                >
                  {desktop.summary}
                </Text>
              </View>
              <View style={styles.trailing}>
                <ChevronRightIcon size={16} color={c.textMuted} />
              </View>
            </Pressable>
            <View
              style={[styles.separator, { backgroundColor: c.borderSubtle }]}
            />
          </>
        )}
        <Pressable
          accessibilityRole="button"
          onPress={onPair}
          style={({ pressed }) => [
            styles.action,
            pressed && { backgroundColor: c.bgRaised },
          ]}
        >
          <Plus size={18} color={c.accentBlue} />
          <Text style={[styles.actionLabel, { color: c.accentBlue }]}>
            Pair a desktop
          </Text>
        </Pressable>
      </View>
    </>
  );
}

const styles = StyleSheet.create({
  heading: {
    marginTop: homeSpacing.xs,
    marginBottom: 7,
    paddingHorizontal: homeSpacing.xs,
    fontSize: 15,
    fontWeight: '600',
  },
  group: { borderRadius: homeRadii.surface, overflow: 'hidden' },
  row: {
    flexDirection: 'row',
    alignItems: 'flex-start',
    paddingLeft: homeSpacing.lg,
  },
  tile: {
    width: 30,
    height: 30,
    marginTop: 9,
    marginRight: 11,
    borderRadius: homeRadii.tile,
    alignItems: 'center',
    justifyContent: 'center',
  },
  dot: {
    position: 'absolute',
    right: -5,
    bottom: -5,
    width: 14,
    height: 14,
    borderRadius: homeRadii.pill,
    borderWidth: 2,
  },
  text: { flex: 1, minWidth: 0, paddingVertical: 9 },
  title: { fontSize: 16, lineHeight: 20 },
  subtitle: { marginTop: 1, fontSize: 14, lineHeight: 18 },
  trailing: { alignSelf: 'center', paddingLeft: 10, paddingRight: 14 },
  separator: { height: StyleSheet.hairlineWidth, marginLeft: homeSpacing.lg },
  action: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 10,
    minHeight: 44,
    paddingHorizontal: homeSpacing.lg,
  },
  actionLabel: { fontSize: 16 },
});
