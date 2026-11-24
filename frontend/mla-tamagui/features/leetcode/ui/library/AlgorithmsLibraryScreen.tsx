import {
  CompactHeader, DenseActionRow, FloatingBar, FloatingSurface, Icon, MarqueeLabel, MenuPanel, SearchPill, useCompactColors, useKeyboardFrame,
} from '@lib/tamagui-design-system';
import { BookOpen, CalendarDays, ChevronLeft, MessageSquare, MoreHorizontal, Network, Play, Search, Share, Star, User } from '@lib/tamagui-design-system/icons';
import { router, useNavigation, type Href } from 'expo-router';
import { useMemo, useRef, useState, type ComponentRef } from 'react';
import { Keyboard, Pressable, ScrollView, StyleSheet, Text, TextInput, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { filterLibrary, type LibraryRow } from '../../domain/library/library';
import { useLibrary } from '../../state/library/useLibrary';
import { useLeetCodePorts } from '../../state/ports';
import { shareProblem } from './libraryShare';
import { SavedListSheets } from './SavedListSheets';

const difficultyText = { easy: 'Easy', medium: 'Med.', hard: 'Hard' } as const;
const tabs = [
  { label: 'Library', icon: BookOpen },
  { label: 'Discuss', icon: MessageSquare },
  { label: 'Calendar', icon: CalendarDays },
  { label: 'You', icon: User },
  { label: 'Search', icon: Search },
];

export function AlgorithmsLibraryScreen() {
  const colors = useCompactColors();
  const navigation = useNavigation();
  const { top, bottom } = useSafeAreaInsets();
  const { libraryContent } = useLeetCodePorts();
  const { lastOpened, actions } = useLibrary();
  const [query, setQuery] = useState('');
  const [searching, setSearching] = useState(false);
  const keyboard = useKeyboardFrame();
  const [saving, setSaving] = useState<LibraryRow>();
  const searchRef = useRef<ComponentRef<typeof TextInput>>(null);
  const filtered = useMemo(() => filterLibrary(libraryContent.rows, query), [libraryContent, query]);
  const resume = lastOpened ? libraryContent.rows.find((row) => row.id === lastOpened) : undefined;
  const dockBottom = Math.max(bottom - 12, 10);
  const dismissKeyboard = () => {
    Keyboard.dismiss();
  };
  const openProblem = (id: string) => {
    dismissKeyboard();
    void actions.openProblem(id);
    router.push(`/leetcode/${id}/problem`);
  };
  const cancelSearch = () => {
    dismissKeyboard();
    setQuery('');
    setSearching(false);
  };
  const back = () => {
    if (router.canGoBack()) router.back();
    else navigation.getParent()?.dispatch({ type: 'OPEN_DRAWER' });
  };

  return (
    <View testID="AlgorithmsLibrary" style={[styles.screen, { backgroundColor: colors.background, paddingTop: top }]}>
      {!searching && <CompactHeader onClose={back} closeIcon={ChevronLeft} trailing={
        <MenuPanel icon={MoreHorizontal} size={42} glassTrigger accessibilityLabel="Library actions" testID="AlgorithmsLibraryMenu"
          panelWidth={220} items={[
            { label: 'Blind 75', icon: BookOpen, testID: 'AlgorithmsBlind75', onPress: () => router.push('/blind75' as Href) },
            { label: 'Settings', icon: User, testID: 'AlgorithmsSettings', onPress: () => router.push('/settings') },
          ]} />
      } />}
      <View style={styles.search}>
        <SearchPill inputRef={searchRef} value={query} onChangeText={setQuery} onFocus={() => setSearching(true)} onSubmit={dismissKeyboard} onCancel={searching ? cancelSearch : undefined} testID="AlgorithmsSearch" />
      </View>
      <ScrollView keyboardShouldPersistTaps="handled" keyboardDismissMode="on-drag" showsVerticalScrollIndicator={false}
        contentContainerStyle={[styles.content, { paddingBottom: 130 + bottom }]}>
        <View style={[styles.hero, searching && styles.searchHero]}>
          <View style={[styles.artwork, { backgroundColor: colors.surface }]}>
            <Icon as={Network} color={colors.gold} size={43} />
          </View>
          <Text style={[styles.title, { color: colors.foreground }]}>Algorithms</Text>
          <Text style={[styles.heroMeta, { color: colors.muted }]}>LeetCode</Text>
          <Text style={[styles.heroMeta, { color: colors.muted }]}>100 saved</Text>
          {query.trim() ? <Text style={[styles.searchCount, { color: colors.muted }]}>{filtered.length}Qts</Text> : (
            <View style={styles.progress}>
              <View style={[styles.progressTrack, { backgroundColor: colors.raised }]}>
                <View style={[styles.progressBar, { backgroundColor: colors.green }]} />
              </View>
              <Text style={{ color: colors.foreground, fontSize: 12 }}>210<Text style={{ color: colors.muted }}>/3,705</Text></Text>
            </View>
          )}
          <Pressable accessibilityRole="button" testID="AlgorithmsPractice" disabled={!filtered.length} onPress={() => openProblem(filtered[0].id)}>
            <FloatingSurface style={styles.practice}>
              <Icon as={Play} size={16} color={colors.green} filled />
              <Text style={{ color: colors.green, fontSize: 16, fontWeight: '700' }}>Practice</Text>
            </FloatingSurface>
          </Pressable>
        </View>
        {filtered.map((row) => (
          <LibraryProblemRow key={row.id} row={row} onOpen={() => openProblem(row.id)} onSave={() => { dismissKeyboard(); setSaving(row); }} />
        ))}
        {!filtered.length && <Text style={[styles.empty, { color: colors.muted }]}>No problems match your search.</Text>}
      </ScrollView>
      {!keyboard.visible && (
        <>
          {resume && <FloatingBar bottom={dockBottom + 72}>
            <Pressable accessibilityRole="button" accessibilityLabel={`Resume ${resume.title}`} testID="AlgorithmsResume" onPress={() => openProblem(resume.id)} style={{ flex: 1 }}>
              <FloatingSurface style={styles.resume} tone="subtle">
                <Icon as={Network} size={21} color={colors.gold} />
                <View style={{ flex: 1 }}>
                  <MarqueeLabel label={`${resume.number}. ${resume.title}`} textStyle={{ color: colors.foreground, fontSize: 14, fontWeight: '600' }} />
                  <Text style={{ color: colors.muted, fontSize: 12 }}>Algorithms</Text>
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
                    testID={`AlgorithmsTab-${label}`} style={styles.tab} onPress={() => {
                      if (label === 'Search') searchRef.current?.focus();
                      else if (label === 'You') router.push('/settings');
                      else cancelSearch();
                    }}>
                    <Icon as={icon} size={20} color={color} />
                    <Text style={{ color, fontSize: 10, fontWeight: '500' }}>{label}</Text>
                  </Pressable>
                );
              })}
            </FloatingSurface>
          </FloatingBar>
        </>
      )}
      {saving && <SavedListSheets problemId={saving.id} onClose={() => setSaving(undefined)} />}
    </View>
  );
}

function LibraryProblemRow({ row, onOpen, onSave }: { row: LibraryRow; onOpen: () => void; onSave: () => void }) {
  const colors = useCompactColors();
  const tint = row.difficulty === 'easy' ? colors.green : row.difficulty === 'medium' ? colors.gold : colors.red;
  return (
    <DenseActionRow density="comfortable" label={`${row.number}. ${row.title}`} testID={`AlgorithmsProblem-${row.id}`} onPress={onOpen}
      trailing={
        <MenuPanel icon={MoreHorizontal} size={34} accessibilityLabel={`Actions for ${row.title}`} testID={`AlgorithmsActions-${row.id}`}
          glass placement="overlap" panelWidth={246} panelRadius={30} items={[
            { label: 'Share', icon: Share, testID: 'AlgorithmsShareProblem', afterDismiss: true, onPress: () => { void shareProblem(row.id, row.title); } },
            { label: 'Add to List', icon: Star, testID: 'AlgorithmsAddProblemToList', onPress: onSave },
          ]} />
      }>
      <View style={styles.rowLine}>
        <Text style={[styles.difficulty, { color: tint, backgroundColor: `${tint}23` }]}>{difficultyText[row.difficulty]}</Text>
        <Text numberOfLines={1} style={[styles.rowTitle, { color: colors.foreground }]}>{row.number}. {row.title}</Text>
      </View>
      <View style={styles.rowLine}>
        <Text style={{ color: row.solved ? colors.green : colors.muted, width: 38, textAlign: 'center', fontSize: 12 }}>{row.solved ? '✓' : '–'}</Text>
        {row.acceptance && <Text style={[styles.metadata, { color: colors.muted }]}>Acceptance {row.acceptance}</Text>}
        {row.frequency && <Text style={[styles.metadata, { color: colors.muted }]}>Frequency {row.frequency}</Text>}
      </View>
    </DenseActionRow>
  );
}

const styles = StyleSheet.create({
  screen: { flex: 1 },
  search: { paddingHorizontal: 18, paddingVertical: 3 },
  content: { paddingHorizontal: 18 },
  hero: { alignItems: 'center', paddingTop: 9, paddingBottom: 14, gap: 4 },
  searchHero: { paddingTop: 5, paddingBottom: 5, gap: 3 },
  artwork: { width: 80, height: 80, borderRadius: 10, alignItems: 'center', justifyContent: 'center', marginBottom: 7 },
  title: { fontSize: 23, fontWeight: '600' },
  heroMeta: { fontSize: 13 },
  searchCount: { fontSize: 12, paddingTop: 8 },
  progress: { flexDirection: 'row', alignItems: 'center', gap: 9, paddingTop: 10 },
  progressTrack: { width: 173, height: 4, borderRadius: 3 },
  progressBar: { width: 10, height: 4, borderRadius: 3 },
  practice: { minWidth: 176, minHeight: 48, borderRadius: 24, marginTop: 10, paddingHorizontal: 32, gap: 6, alignItems: 'center', justifyContent: 'center', flexDirection: 'row' },
  rowLine: { flexDirection: 'row', alignItems: 'center', gap: 9 },
  difficulty: { paddingHorizontal: 4, paddingVertical: 1, borderRadius: 3, minWidth: 38, textAlign: 'center', fontSize: 11, fontWeight: '700' },
  rowTitle: { flex: 1, fontSize: 15, fontWeight: '600' },
  metadata: { fontSize: 11, marginRight: 8 },
  empty: { textAlign: 'center', marginTop: 28 },
  resume: { minHeight: 46, borderRadius: 24, flexDirection: 'row', alignItems: 'center', gap: 9, paddingHorizontal: 14 },
  tabBar: { flex: 1, flexDirection: 'row', alignItems: 'center', paddingHorizontal: 10, borderRadius: 29, minHeight: 60 },
  tab: { flex: 1, minHeight: 48, justifyContent: 'center', alignItems: 'center', gap: 2 },
});
