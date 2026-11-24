import { libraryArtwork } from './libraryArtwork';
import { CompactIconButton, Icon, useCompactColors } from '@lib/tamagui-design-system';
import { BookOpen, ChevronRight, Code, Menu, Network } from '@lib/tamagui-design-system/icons';
import { router, useNavigation, type Href } from 'expo-router';
import { useMemo, useRef, useState } from 'react';
import { Image, Linking, Pressable, ScrollView, StyleSheet, Text, View, type ScrollViewInstance } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { useLeetCodePorts } from '../../state/ports';
import { LibraryDock } from './LibraryDock';

const categories = [
  { name: 'All Topics', icon: BookOpen, color: '#c790eb' },
  { name: 'Algorithms', icon: Network, color: '#ffa318' },
  { name: 'Database', icon: BookOpen, color: '#55bbff' },
  { name: 'NeetCode 150', icon: Code, color: '#43ceb2' },
  { name: 'Blind 75', icon: Code, color: '#cf97f4' },
];
const firstTopics = [
  ['Array', 'Math', 'Greedy', 'Depth-First Search'],
  ['String', 'Dynamic Programming', 'Database', 'Matrix', 'Tree', 'Graph'],
  ['Hash Table', 'Sorting', 'Binary Search', 'Bit Manipulation'],
];

function openCollection(name: string, search = false) {
  router.push({ pathname: '/leetcode/library/collection', params: { collection: name, ...(search ? { search: '1' } : {}) } } as Href);
}

export function LibraryHomeScreen() {
  const colors = useCompactColors();
  const { top, bottom } = useSafeAreaInsets();
  const [width, setWidth] = useState(390);
  const scroll = useRef<ScrollViewInstance>(null);
  const navigation = useNavigation();
  const { libraryContent } = useLeetCodePorts();
  const [expanded, setExpanded] = useState(false);
  const groups = useMemo(() => Array.from(new Set(libraryContent.studyPlan.map((entry) => entry.group))), [libraryContent]);
  const bannerWidth = Math.round(width * 0.835);
  const select = (name: string) => {
    if (name === 'Blind 75') router.push('/blind75' as Href);
    else if (name === 'Database') void Linking.openURL('https://leetcode.com/problemset/database/');
    else openCollection(name);
  };
  const heading = (name: string) => <Text accessibilityRole="header" style={[styles.section, { color: colors.foreground }]}>{name}</Text>;
  const chip = (name: string) => <Pressable key={name} accessibilityRole="button" testID={`LibraryTopic-${name}`} onPress={() => select(name)}
    style={[styles.chip, { backgroundColor: colors.surface }]}><Text style={{ color: colors.muted, fontSize: 15, fontWeight: '600' }}>{name}</Text></Pressable>;

  return <View testID="LibraryHome" onLayout={({ nativeEvent }) => {
    if (nativeEvent.layout.width > 0) setWidth(nativeEvent.layout.width);
  }} style={{ flex: 1, backgroundColor: colors.background, paddingTop: top }}>
    <View style={styles.header}>
      <CompactIconButton label="Open app menu" icon={Menu} testID="LibraryHomeMenuButton" onPress={() => navigation.dispatch({ type: 'OPEN_DRAWER' })} />
      <Text accessibilityRole="header" style={[styles.title, { color: colors.foreground }]}>Library</Text>
    </View>
    <ScrollView ref={scroll} showsVerticalScrollIndicator={false} contentContainerStyle={{ paddingBottom: bottom + 150 }}>
      <ScrollView horizontal showsHorizontalScrollIndicator={false} decelerationRate="fast" snapToInterval={bannerWidth + 10}
        contentContainerStyle={styles.rail}>
        <Pressable accessibilityRole="button" accessibilityLabel="Weekly Contest" onPress={() => router.push('/leetcode/library/contest' as Href)}
          style={[styles.banner, { width: bannerWidth, height: bannerWidth / 2 }]}>
          <Image source={libraryArtwork.contest} style={{ position: 'absolute', width: bannerWidth, height: bannerWidth / 2 }} resizeMode="cover" />
        </Pressable>
        <Pressable accessibilityRole="button" accessibilityLabel="NeetCode 150 study plan" onPress={() => openCollection('NeetCode 150')}
          style={[styles.banner, { width: bannerWidth, height: bannerWidth / 2, backgroundColor: '#ffc56b' }]}>
          <Icon as={Network} color="#75460d" size={64} /><Text style={styles.bannerLabel}>NeetCode 150</Text>
        </Pressable>
      </ScrollView>
      {heading('Category')}
      <ScrollView horizontal showsHorizontalScrollIndicator={false} contentContainerStyle={styles.rail}>
        {categories.map(({ name, icon, color }) => <Pressable accessibilityRole="button" key={name} testID={`LibraryCategory-${name}`}
          onPress={() => select(name)} style={[styles.tile, { width: width * 0.322, backgroundColor: colors.background, borderColor: colors.line }]}>
          <View pointerEvents="none" style={[StyleSheet.absoluteFill, { backgroundColor: color, opacity: 0.11 }]} />
          <Icon as={icon} size={26} color={color} /><Text style={[styles.tileLabel, { color: colors.foreground }]}>{name}</Text>
        </Pressable>)}
      </ScrollView>
      <Pressable accessibilityRole="button" accessibilityState={{ expanded }} onPress={() => setExpanded(!expanded)} style={styles.sectionLink}>
        {heading('Knowledge')}<Icon as={ChevronRight} color={colors.muted} size={20} />
      </Pressable>
      <ScrollView horizontal showsHorizontalScrollIndicator={false} contentContainerStyle={styles.rail}>
        <View style={{ gap: 8 }}>{firstTopics.map((topics) => <View key={topics[0]} style={styles.chipRow}>{topics.map(chip)}</View>)}</View>
      </ScrollView>
      {expanded && <View style={styles.expanded}>{groups.filter((name) => !firstTopics.flat().includes(name)).map(chip)}</View>}
    </ScrollView>
    <LibraryDock onSearch={() => openCollection('All Topics', true)} onLibrary={() => scroll.current?.scrollTo({ y: 0 })} />
  </View>;
}

const styles = StyleSheet.create({
  header: { paddingHorizontal: 20, marginBottom: 12, flexDirection: 'row', alignItems: 'center', gap: 12 },
  title: { fontSize: 34, letterSpacing: -0.8, fontWeight: '700' },
  rail: { paddingHorizontal: 20, gap: 10 },
  banner: { borderRadius: 14, overflow: 'hidden', padding: 22, justifyContent: 'flex-end', backgroundColor: '#ffae39' },
  bannerLabel: { fontSize: 36, lineHeight: 39, color: '#34210e', letterSpacing: -1.1, fontWeight: '600' },
  section: { marginLeft: 20, marginTop: 22, marginBottom: 15, fontSize: 22, fontWeight: '700', letterSpacing: -0.5 },
  sectionLink: { marginTop: 21, flexDirection: 'row', alignItems: 'baseline', gap: 4 },
  tile: { justifyContent: 'space-between', height: 82, borderRadius: 12, borderWidth: 1, padding: 12, paddingHorizontal: 15, overflow: 'hidden' },
  tileLabel: { fontSize: 16, fontWeight: '600' },
  chipRow: { flexDirection: 'row', gap: 8 },
  chip: { paddingHorizontal: 13, paddingVertical: 6, borderRadius: 16 },
  expanded: { flexDirection: 'row', flexWrap: 'wrap', paddingHorizontal: 20, paddingTop: 8, gap: 8 },
});
