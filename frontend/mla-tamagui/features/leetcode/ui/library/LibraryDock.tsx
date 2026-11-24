import { FloatingBar, FloatingSurface, Icon, MarqueeLabel, useCompactColors } from '@lib/tamagui-design-system';
import { BookOpen, CalendarDays, MessageSquare, Network, Play, Search, User } from '@lib/tamagui-design-system/icons';
import { router } from 'expo-router';
import { Pressable, StyleSheet, Text, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { useLibrary } from '../../state/library/useLibrary';
import { useLeetCodePorts } from '../../state/ports';

const tabs = [
  { label: 'Library', icon: BookOpen },
  { label: 'Discuss', icon: MessageSquare },
  { label: 'Calendar', icon: CalendarDays },
  { label: 'You', icon: User },
  { label: 'Search', icon: Search },
];

export function LibraryDock({ onLibrary, onSearch }: { onLibrary: () => void; onSearch: () => void }) {
  const colors = useCompactColors();
  const { bottom } = useSafeAreaInsets();
  const { libraryContent } = useLeetCodePorts();
  const { lastOpened } = useLibrary();
  const resume = libraryContent.rows.find((row) => row.id === lastOpened);
  const dockBottom = Math.max(bottom - 11, 10);
  return <>
          {resume && <FloatingBar bottom={dockBottom + 68}>
            <Pressable accessibilityRole="button" accessibilityLabel={`Resume ${resume.title}`} testID="AlgorithmsResume" onPress={() => router.push(`/leetcode/${resume.id}/problem`)} style={{ flex: 1 }}>
              <FloatingSurface style={styles.resume} tone="subtle">
                <View style={[styles.resumeIcon, { backgroundColor: colors.surface }]}><Icon as={Network} size={18} color={colors.gold} /></View>
                <View style={{ flex: 1 }}>
                  <MarqueeLabel label={`${resume.number}. ${resume.title}`} textStyle={{ color: colors.foreground, fontSize: 16, fontWeight: '600' }} />
                  <Text style={{ color: colors.muted, fontSize: 13 }}>Algorithms</Text>
                </View>
                <Icon as={Play} size={16} color={colors.foreground} filled />
              </FloatingSurface>
            </Pressable>
          </FloatingBar>}
          <FloatingBar bottom={dockBottom}>
            <FloatingSurface testID="AlgorithmsTabBar" tone="subtle" style={styles.tabBar}>
              {tabs.map(({ label, icon }) => {
                const selected = label === 'Library';
                const disabled = label === 'Discuss' || label === 'Calendar';
                const color = selected ? colors.gold : colors.foreground;
                return (
                  <Pressable key={label} accessibilityRole="tab" accessibilityState={{ selected, disabled }} accessibilityLabel={label} disabled={disabled}
                    testID={`AlgorithmsTab-${label}`} style={[styles.tab, selected && { backgroundColor: colors.background }]} onPress={() => {
                      if (label === 'Search') onSearch();
                      else if (label === 'You') router.push('/settings');
                      else onLibrary();
                    }}>
                    <Icon as={icon} size={20} color={color} />
                    <Text style={{ color, fontSize: 10, fontWeight: '500' }}>{label}</Text>
                  </Pressable>
                );
              })}
            </FloatingSurface>
          </FloatingBar>
  </>;
}

const styles = StyleSheet.create({
  resumeIcon: { height: 31, width: 31, borderRadius: 7, alignItems: 'center', justifyContent: 'center' },
  resume: { minHeight: 48, borderRadius: 24, flexDirection: 'row', alignItems: 'center', gap: 9, paddingLeft: 10, paddingRight: 20 },
  tabBar: { flex: 1, flexDirection: 'row', alignItems: 'center', paddingHorizontal: 4, borderRadius: 29, minHeight: 60 },
  tab: { flex: 1, minHeight: 52, borderRadius: 26, justifyContent: 'center', alignItems: 'center', gap: 2 },
});
