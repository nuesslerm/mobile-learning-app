import { useRouteNavigation } from '../useRouteAction';
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
import { usePrefetchProblem } from './usePrefetchProblem';
import { LibraryDock } from './LibraryDock';
import { libraryArtwork } from './libraryArtwork';
import { useLibrary } from '../../state/library/useLibrary';
import { shareProblem } from './libraryShare';
import { SavedListSheets } from './SavedListSheets';

const difficultyText = { easy: 'Easy', medium: 'Med.', hard: 'Hard' } as const;
const collectionArtwork = new Map([
  ['All Topics', libraryArtwork.allTopics],
  ['Depth-First Search', libraryArtwork.depthFirst],
  ['Dynamic Programming', libraryArtwork.dynamicProgramming],
]);

export function AlgorithmsLibraryScreen({ collection, focusSearch = false }: { collection: string; focusSearch?: boolean }) {
  const { leave, pushRoute } = useRouteNavigation();
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
  usePrefetchProblem(query ? undefined : filtered[0]?.id);
  const { actions, savedCollections } = useLibrary();
  const saved = savedCollections?.includes(collection);
  const artwork = collectionArtwork.get(collection);
  const isTopic = collection === 'Depth-First Search' || collection === 'Dynamic Programming';
  const isAlgorithms = collection === 'Algorithms';
  const accent = isAlgorithms ? colors.gold : collection === 'Dynamic Programming' ? colors.green : '#ac84f3';
  const dismissKeyboard = () => {
    Keyboard.dismiss();
  };
  const openProblem = (id: string) => {
    dismissKeyboard();
    pushRoute(`/leetcode/${id}/problem`);
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
    if (router.canGoBack()) leave(() => router.back());
    else navigation.getParent()?.dispatch({ type: 'OPEN_DRAWER' });
  };

  return (
    <View testID="AlgorithmsLibrary" style={[styles.screen, { backgroundColor: colors.background, paddingTop: artwork ? Math.max(0, top - 12) : top }]}>
      {!searching && <CompactHeader onClose={back} closeIcon={ChevronLeft} trailing={
        <View style={styles.headerActions}>
        {isTopic && <CompactIconButton label={saved ? 'Unsave collection' : 'Save collection'} icon={Star} filled={saved} onPress={() => { void actions.toggleCollection(collection); }} />}
        <MenuPanel icon={MoreHorizontal} size={42} glassTrigger accessibilityLabel="Library actions" testID="AlgorithmsLibraryMenu"
          panelWidth={220} items={[
            { label: 'Blind 75', icon: BookOpen, testID: 'AlgorithmsBlind75', onPress: () => pushRoute('/blind75' as Href) },
            { label: 'Settings', icon: User, testID: 'AlgorithmsSettings', onPress: () => pushRoute('/settings') },
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
            {artwork ? <Image source={artwork} style={{ width: 80, height: 80, borderRadius: 10 }} /> : <Icon as={Network} color={accent} size={43} />}
          </View>
          <Text style={[styles.title, { color: colors.foreground }]}>{collection}</Text>
          <Text style={[styles.heroMeta, { color: colors.muted }]}>{collection === 'NeetCode 150' ? 'NeetCode' : 'LeetCode'}</Text>
          <Text style={[styles.heroMeta, { color: colors.muted }]}>{rows.length} {rows.length === 1 ? 'problem' : 'problems'}</Text>
          {!!query.trim() && <Text style={[styles.searchCount, { color: colors.muted }]}>{filtered.length} {filtered.length === 1 ? 'match' : 'matches'}</Text>}
          <Pressable accessibilityRole="button" testID="AlgorithmsPractice" disabled={!filtered.length} onPress={() => openProblem(filtered[0].id)}>
            <FloatingSurface style={styles.practice}>
              <Icon as={Play} size={16} color={colors.green} filled />
              <Text style={{ color: colors.green, fontSize: 16, fontWeight: '700' }}>Practice</Text>
            </FloatingSurface>
          </Pressable>
        </View>
        } />
      {!keyboard.visible && <LibraryDock pushRoute={pushRoute} onSearch={() => searchRef.current?.focus()} onLibrary={() => leave(() => router.dismissTo('/'))} />}
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
      {topic && <Text style={[styles.metadata, { color: colors.muted }]}>{topic}</Text>}
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
  practice: { minWidth: 176, minHeight: 48, borderRadius: 24, marginTop: 10, paddingHorizontal: 32, gap: 6, alignItems: 'center', justifyContent: 'center', flexDirection: 'row' },
  rowLine: { flexDirection: 'row', alignItems: 'center', gap: 9 },
  difficulty: { paddingHorizontal: 4, paddingVertical: 1, borderRadius: 3, minWidth: 38, textAlign: 'center', fontSize: 11, fontWeight: '700' },
  rowTitle: { flex: 1, fontSize: 15, fontWeight: '600' },
  metadata: { fontSize: 11, marginRight: 8 },
  empty: { textAlign: 'center', marginTop: 28 },

});
