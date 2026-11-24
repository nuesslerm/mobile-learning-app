import {
  CompactHeader, CompactIconButton, DenseActionRow, FloatingSurface, Icon, MenuPanel, SearchPill, useCompactColors, useKeyboardFrame,
} from '@lib/tamagui-design-system';
import { BookOpen, ChevronLeft, MoreHorizontal, Network, Play, Share, Star, User } from '@lib/tamagui-design-system/icons';
import { router, useNavigation, type Href } from 'expo-router';
import { useEffect, useMemo, useRef, useState, type ComponentRef } from 'react';
import { FlatList, Image, Keyboard, Pressable, StyleSheet, Text, TextInput, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { filterLibrary, type LibraryRow } from '../../domain/library/library';
import { useLeetCodePorts } from '../../state/ports';
import { collectionRows } from '../../domain/library/collections';
import { LibraryDock } from './LibraryDock';
import { libraryArtwork } from './libraryArtwork';
import { useLibrary } from '../../state/library/useLibrary';
import { shareProblem } from './libraryShare';
import { SavedListSheets } from './SavedListSheets';

const difficultyText = { easy: 'Easy', medium: 'Med.', hard: 'Hard' } as const;
const recordedCollections = new Map([
  ['All Topics', { icon: libraryArtwork.allTopics, saved: '100', solved: 213, total: '4,123' }],
  ['Depth-First Search', { icon: libraryArtwork.depthFirst, saved: '4,388', solved: 35, total: '351' }],
  ['Dynamic Programming', { icon: libraryArtwork.dynamicProgramming, saved: '14,116', solved: 41, total: '687' }],
]);

export function AlgorithmsLibraryScreen({ collection, focusSearch = false }: { collection: string; focusSearch?: boolean }) {
  const colors = useCompactColors();
  const navigation = useNavigation();
  const { top, bottom } = useSafeAreaInsets();
  const { libraryContent } = useLeetCodePorts();
  const [query, setQuery] = useState('');
  const [searching, setSearching] = useState(false);
  const keyboard = useKeyboardFrame();
  const [saving, setSaving] = useState<LibraryRow>();
  const searchRef = useRef<ComponentRef<typeof TextInput>>(null);
  const rows = useMemo(() => collectionRows(libraryContent, collection), [libraryContent, collection]);
  const filtered = useMemo(() => filterLibrary(rows, query), [rows, query]);
  const solved = useMemo(() => rows.filter((row) => row.solved).length, [rows]);
  const { actions, savedCollections } = useLibrary();
  const saved = savedCollections?.includes(collection);
  const recorded = recordedCollections.get(collection);
  const isTopic = collection === 'Depth-First Search' || collection === 'Dynamic Programming';
  const isAlgorithms = collection === 'Algorithms';
  const accent = isAlgorithms ? colors.gold : collection === 'Dynamic Programming' ? colors.green : '#ac84f3';
  const dismissKeyboard = () => {
    Keyboard.dismiss();
  };
  const openProblem = (id: string) => {
    dismissKeyboard();
    router.push(`/leetcode/${id}/problem`);
  };
  useEffect(() => {
    if (focusSearch) searchRef.current?.focus();
  }, [focusSearch]);
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
    <View testID="AlgorithmsLibrary" style={[styles.screen, { backgroundColor: colors.background, paddingTop: recorded ? Math.max(0, top - 12) : top }]}>
      {!searching && <CompactHeader onClose={back} closeIcon={ChevronLeft} trailing={
        <View style={styles.headerActions}>
        {isTopic && <CompactIconButton label={saved ? 'Unsave collection' : 'Save collection'} icon={Star} filled={saved} onPress={() => { void actions.toggleCollection(collection); }} />}
        <MenuPanel icon={MoreHorizontal} size={42} glassTrigger accessibilityLabel="Library actions" testID="AlgorithmsLibraryMenu"
          panelWidth={220} items={[
            { label: 'Blind 75', icon: BookOpen, testID: 'AlgorithmsBlind75', onPress: () => router.push('/blind75' as Href) },
            { label: 'Settings', icon: User, testID: 'AlgorithmsSettings', onPress: () => router.push('/settings') },
          ]} />
        </View>
      } />}
      <View style={styles.search}>
        <SearchPill inputRef={searchRef} value={query} onChangeText={setQuery} onFocus={() => setSearching(true)} onSubmit={dismissKeyboard} onCancel={searching ? cancelSearch : undefined} testID="AlgorithmsSearch" />
      </View>
      <FlatList data={filtered} keyExtractor={(row) => row.id} initialNumToRender={9} windowSize={5}
        keyboardShouldPersistTaps="handled" keyboardDismissMode="on-drag" showsVerticalScrollIndicator={false}
        contentContainerStyle={[styles.content, { paddingBottom: 148 + bottom }]}
        renderItem={({ item }) => <LibraryProblemRow row={item} topic={collection === 'NeetCode 150' ? libraryContent.studyPlan.find((entry) => entry.id === item.id)?.group : undefined} onOpen={() => openProblem(item.id)} onSave={() => { dismissKeyboard(); setSaving(item); }} />}
        ListEmptyComponent={<Text style={[styles.empty, { color: colors.muted }]}>No problems match your search.</Text>}
        ListHeaderComponent={
        <View style={[styles.hero, searching && styles.searchHero]}>
          <View style={[styles.artwork, { backgroundColor: colors.surface }]}>
            {recorded ? <Image source={recorded.icon} style={{ width: 80, height: 80, borderRadius: 10 }} /> : <Icon as={Network} color={accent} size={43} />}
          </View>
          <Text style={[styles.title, { color: colors.foreground }]}>{collection}</Text>
          <Text style={[styles.heroMeta, { color: colors.muted }]}>{collection === 'NeetCode 150' ? 'NeetCode' : 'LeetCode'}</Text>
          <Text style={[styles.heroMeta, { color: colors.muted }]}>{recorded ? `${recorded.saved} saved` : isAlgorithms ? '100 saved' : `${rows.length} problems`}</Text>
          {query.trim() ? <Text style={[styles.searchCount, { color: colors.muted }]}>{filtered.length} {filtered.length === 1 ? 'question' : 'questions'}</Text> : (
            <View style={styles.progress}>
              <View style={[styles.progressTrack, { backgroundColor: colors.raised }]}>
                <View style={[styles.progressBar, { backgroundColor: colors.green, width: recorded ? 173 * recorded.solved / Number(recorded.total.replace(',', '')) : isAlgorithms ? 10 : 173 * solved / Math.max(rows.length, 1) }]} />
              </View>
              <Text style={{ color: colors.foreground, fontSize: 12 }}>{recorded?.solved ?? (isAlgorithms ? 210 : solved)}<Text style={{ color: colors.muted }}>/{recorded?.total ?? (isAlgorithms ? '3,705' : rows.length)}</Text></Text>
            </View>
          )}
          <Pressable accessibilityRole="button" testID="AlgorithmsPractice" disabled={!filtered.length} onPress={() => openProblem(filtered[0].id)}>
            <FloatingSurface style={styles.practice}>
              <Icon as={Play} size={16} color={colors.green} filled />
              <Text style={{ color: colors.green, fontSize: 16, fontWeight: '700' }}>Practice</Text>
            </FloatingSurface>
          </Pressable>
          {isTopic && <Text style={{ alignSelf: 'flex-start', color: colors.muted, marginTop: 22, fontSize: 13 }}>The list updated 1 weeks ago.</Text>}
        </View>
        } />
      {!keyboard.visible && <LibraryDock onSearch={() => searchRef.current?.focus()} onLibrary={() => router.dismissTo('/')} />}
      {saving && <SavedListSheets problemId={saving.id} onClose={() => setSaving(undefined)} />}
    </View>
  );
}

function LibraryProblemRow({ row, topic, onOpen, onSave }: { row: LibraryRow; topic?: string; onOpen: () => void; onSave: () => void }) {
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
        {topic && <Text style={[styles.metadata, { color: colors.muted }]}>{topic}</Text>}
        {row.acceptance && <Text style={[styles.metadata, { color: colors.muted }]}>Acceptance {row.acceptance}</Text>}
        {row.frequency && <Text style={[styles.metadata, { color: colors.muted }]}>Frequency {row.frequency}</Text>}
      </View>
    </DenseActionRow>
  );
}

const styles = StyleSheet.create({
  screen: { flex: 1 },
  headerActions: { flexDirection: 'row', alignItems: 'center', gap: 4 },
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

});
