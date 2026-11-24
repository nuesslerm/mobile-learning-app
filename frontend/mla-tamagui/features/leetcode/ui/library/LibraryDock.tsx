import { FloatingBar, FloatingSurface, Icon, MarqueeLabel, useCompactColors } from '@lib/tamagui-design-system';
import { BookOpen, CalendarDays, MessageSquare, Play, Search, User } from '@lib/tamagui-design-system/icons';
import { router } from 'expo-router';
import { Image, Pressable, StyleSheet, Text, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { libraryArtwork } from './libraryArtwork';
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
  const dockBottom = Math.max(bottom - 14, 10);
  return <>
          {resume && <FloatingBar style={styles.bar} bottom={dockBottom + 74}>
            <Pressable accessibilityRole="button" accessibilityLabel={`Resume ${resume.title}`} testID="AlgorithmsResume" onPress={() => router.push(`/leetcode/${resume.id}/problem`)} style={{ flex: 1 }}>
              <FloatingSurface style={styles.resume} tone="overlay">
                <Image source={libraryArtwork.resume} style={styles.resumeIcon} />
                <View style={{ flex: 1 }}>
                  <MarqueeLabel pauseMs={3200} label={`${resume.number}. ${resume.title}`} textStyle={{ color: colors.foreground, fontSize: 16, fontWeight: '600' }} />
                  <Text style={{ color: colors.muted, fontSize: 13 }}>Algorithms</Text>
                </View>
                <Icon as={Play} size={16} color={colors.foreground} filled />
              </FloatingSurface>
            </Pressable>
          </FloatingBar>}
          <FloatingBar style={styles.bar} bottom={dockBottom}>
            <FloatingSurface testID="AlgorithmsTabBar" tone="overlay" style={styles.tabBar}>
              {tabs.map(({ label, icon }) => {
                const selected = label === 'Library';
                const disabled = label === 'Discuss' || label === 'Calendar';
                const color = selected ? colors.gold : colors.foreground;
                return (
                  <Pressable key={label} accessibilityRole="tab" accessibilityState={{ selected, disabled }} accessibilityLabel={label} disabled={disabled}
                    testID={`AlgorithmsTab-${label}`} style={styles.tab} onPress={() => {
                      if (label === 'Search') onSearch();
                      else if (label === 'You') router.push('/settings');
                      else onLibrary();
                    }}>
                    {selected && <View pointerEvents="none" style={[StyleSheet.absoluteFill, { borderRadius: 26, backgroundColor: colors.background, opacity: 0.55 }]} />}
                    {label === 'You' ? <Image source={libraryArtwork.profile} style={styles.profile} /> :
                      label === 'Discuss' ? <View style={styles.discussion}><Icon as={icon} size={16} color={color} /><View style={[styles.bubble, { backgroundColor: colors.surface }]}><Icon as={icon} size={16} color={color} /></View></View> :
                      <Icon as={icon} size={23} color={color} />}
                    <Text style={{ color, fontSize: 11, fontWeight: '500' }}>{label}</Text>
                  </Pressable>
                );
              })}
            </FloatingSurface>
          </FloatingBar>
  </>;
}

const styles = StyleSheet.create({
  bar: { left: 21, right: 21 },
  profile: { width: 25, height: 25, borderRadius: 13 },
  discussion: { width: 25, height: 23, paddingTop: 1 },
  bubble: { position: 'absolute', left: 8, top: 5, borderRadius: 4 },
  resumeIcon: { height: 31, width: 31, borderRadius: 6 },
  resume: { minHeight: 48, borderRadius: 24, flexDirection: 'row', alignItems: 'center', gap: 10, paddingLeft: 14, paddingRight: 20 },
  tabBar: { flex: 1, flexDirection: 'row', alignItems: 'center', paddingHorizontal: 8, borderRadius: 32, minHeight: 64 },
  tab: { flex: 1, minHeight: 52, borderRadius: 26, justifyContent: 'center', alignItems: 'center', gap: 2 },
});
