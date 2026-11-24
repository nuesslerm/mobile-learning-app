import { CompactHeader, CompactIconButton, CompactTabs, FloatingBar, Icon, useCompactColors } from '@lib/tamagui-design-system';
import { ChevronDown, Lightbulb, Play, Share, Star, Tag } from '@lib/tamagui-design-system/icons';
import { Link, router } from 'expo-router';
import { useEffect, useMemo, useState } from 'react';
import { Platform, Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import type { Blind75Problem } from '../../domain/models';
import { useLibrary } from '../../state/library/useLibrary';
import { useLeetCodePorts } from '../../state/ports';
import { NeetCodeLesson } from './NeetCodeLesson';
import { problemUrl, shareProblem } from './libraryShare';
import { ProblemInformationSheet, type ProblemInformation } from './ProblemInformationSheets';

const tabs = [
  { id: 'description', label: 'Description' },
  { id: 'lesson', label: 'NeetCode lesson' },
] as const;

export function ProblemDetailScreen({ id }: { id: string }) {
  const colors = useCompactColors();
  const { top, bottom } = useSafeAreaInsets();
  const { libraryContent, editorBridge } = useLeetCodePorts();
  const problem = libraryContent.problem(id);
  const { lists, actions } = useLibrary();
  const favorited = lists.some((list) => list.id === 'favorite' && list.problemIds.includes(id));
  const [tab, setTab] = useState<(typeof tabs)[number]['id']>('description');
  const [information, setInformation] = useState<ProblemInformation>();
  const [scrolled, setScrolled] = useState(false);
  const title = problem ? `${problem.leetcodeId}. ${problem.title}` : 'Problem';
  const study = libraryContent.studyPlan.find((entry) => entry.id === id);
  const lesson = useMemo(() => tab === 'lesson' ? libraryContent.lesson(id) : undefined, [id, libraryContent, tab]);
  const hasProblem = Boolean(problem);
  useEffect(() => {
    if (hasProblem) void actions.openProblem(id);
  }, [actions, hasProblem, id]);
  useEffect(() => {
    if (Platform.OS !== 'ios' || !hasProblem) return;
    const work = requestIdleCallback(() => editorBridge.pageSource?.(editorBridge.html()));
    return () => cancelIdleCallback(work);
  }, [editorBridge, hasProblem]);

  return <View testID="AlgorithmsProblemDetail" style={{ flex: 1, backgroundColor: colors.background, paddingTop: top }}>
    <CompactHeader title={scrolled ? title : undefined} closeIcon={ChevronDown} closeLabel="Close problem"
      onClose={() => router.canGoBack() ? router.back() : router.replace('/')} trailing={problem && <View style={styles.headerActions}>
        <CompactIconButton icon={Star} label={favorited ? 'Remove favorite' : 'Add favorite'} testID="ProblemFavorite"
          color={favorited ? colors.gold : undefined} filled={favorited} onPress={() => { void actions.favorite(id); }} />
        <CompactIconButton icon={Share} label="Share problem" testID="ProblemShare" onPress={() => { void shareProblem(id, title); }} />
      </View>} />
    {problem && study && <CompactTabs items={tabs} value={tab} onChange={(next) => { setScrolled(false); setTab(next); }} testID="ProblemTab" />}
    {tab === 'lesson' && lesson ? <NeetCodeLesson title={problem?.title ?? title} lesson={lesson} /> : <>
      <ScrollView testID="ProblemDescriptionContent" scrollEventThrottle={32}
        onScroll={({ nativeEvent }) => setScrolled(nativeEvent.contentOffset.y > 45)}
        contentContainerStyle={[styles.content, { paddingBottom: bottom + 89 }]}>
        <Text accessibilityRole="header" style={[styles.title, { color: colors.foreground }]}>{title}</Text>
        {problem ? <>
          {study && <Pressable accessibilityRole="button" testID="ProblemOpenLesson" onPress={() => setTab('lesson')}
            style={[styles.lessonLink, { backgroundColor: colors.surface }]}>
            <Text style={{ color: colors.green, fontWeight: '600' }}>NeetCode 150 · {study.group}</Text>
            <Text style={{ color: colors.foreground }}>View lesson ›</Text>
          </Pressable>}
          <Description problem={problem} onInformation={setInformation} />
          <View style={[styles.sources, { borderColor: colors.line }]}>
            <Link href={problemUrl(id)} style={{ color: colors.green }}>Problem on LeetCode ↗</Link>
            <Link testID="ProblemCommunitySolutions" href={`${problemUrl(id)}solutions/`} style={{ color: colors.green }}>Community solutions on LeetCode ↗</Link>
            <Link href={`${problemUrl(id)}submissions/`} style={{ color: colors.green }}>My submissions on LeetCode ↗</Link>
          </View>
        </> : <Text style={{ color: colors.muted }}>This problem is unavailable in the local collection.</Text>}
      </ScrollView>
      {problem && <FloatingBar bottom={bottom + 7} style={{ justifyContent: 'flex-end' }}>
        <CompactIconButton icon={Play} label="Open editor" testID="ProblemOpenEditor" emphasis filled
          onPress={() => router.push({ pathname: '/leetcode/[id]', params: { id, lang: 'javascript' } })} />
      </FloatingBar>}
    </>}
    {information && problem && <ProblemInformationSheet kind={information} topics={problem.topics} hints={problem.hints} onClose={() => setInformation(undefined)} />}
  </View>;
}

function Description({ problem, onInformation }: { problem: Blind75Problem; onInformation: (value: ProblemInformation) => void }) {
  const colors = useCompactColors();
  const difficultyColor = problem.difficulty === 'easy' ? colors.green : problem.difficulty === 'medium' ? colors.gold : colors.red;
  const examples = problem.examples.split(/(```[\s\S]*?```)/).filter((part) => part.trim());
  return <>
    <View style={styles.chips}>
      <Text style={[styles.chip, { color: difficultyColor, backgroundColor: `${difficultyColor}23` }]}>{problem.difficulty[0].toUpperCase() + problem.difficulty.slice(1)}</Text>
      {([['topics', 'Topics', Tag, problem.topics], ['hints', 'Hints', Lightbulb, problem.hints]] as const).map(([kind, label, icon, entries]) => entries.length > 0 &&
        <Pressable key={kind} accessibilityRole="button" testID={`ProblemInfoOpen-${kind}`} onPress={() => onInformation(kind)}
          style={[styles.chipButton, { backgroundColor: colors.surface }]}>
          <Icon as={icon} size={12} color={colors.foreground} /><Text style={{ color: colors.foreground, fontSize: 12 }}>{label}</Text>
        </Pressable>)}
    </View>
    <Text style={[styles.paragraph, { color: colors.foreground }]}>{problem.description}</Text>
    {examples.map((part, index) => part.startsWith('```') ?
      <Text key={index} selectable style={[styles.code, { color: colors.foreground, borderColor: colors.raised }]}>{part.slice(3, -3).trim()}</Text> :
      <Text key={index} style={[styles.exampleTitle, { color: colors.foreground }]}>{part.trim().replaceAll('**', '')}</Text>)}
    {!!problem.constraints && <>
      <Text style={[styles.exampleTitle, { color: colors.foreground }]}>Constraints</Text>
      <Text style={[styles.paragraph, { color: colors.foreground }]}>{problem.constraints.replace(/^- /gm, '•  ')}</Text>
    </>}
  </>;
}

const styles = StyleSheet.create({
  lessonLink: { marginBottom: 12, padding: 12, borderRadius: 12, flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', gap: 8, flexWrap: 'wrap' },
  headerActions: { flexDirection: 'row', gap: 2 },
  content: { paddingHorizontal: 18, paddingTop: 14 },
  title: { fontSize: 22, fontWeight: '600', marginBottom: 12 },
  chips: { flexDirection: 'row', alignItems: 'center', gap: 6, flexWrap: 'wrap' },
  chip: { fontSize: 12, borderRadius: 12, overflow: 'hidden', paddingHorizontal: 8, paddingVertical: 4 },
  chipButton: { flexDirection: 'row', alignItems: 'center', gap: 4, borderRadius: 12, paddingHorizontal: 8, minHeight: 23 },
  paragraph: { fontSize: 16, lineHeight: 21, marginTop: 14 },
  exampleTitle: { fontSize: 15, fontWeight: '700', marginTop: 22, marginBottom: 8 },
  code: { fontFamily: 'monospace', fontSize: 13, lineHeight: 18, paddingLeft: 11, borderLeftWidth: 2 },
  sources: { gap: 16, marginTop: 24, paddingTop: 16, borderTopWidth: StyleSheet.hairlineWidth },
});
