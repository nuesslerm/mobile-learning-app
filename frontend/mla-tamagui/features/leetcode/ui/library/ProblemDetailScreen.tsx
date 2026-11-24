import {
  CompactHeader, CompactIconButton, CompactTabs, DenseActionRow, FloatingBar, FloatingSurface, Icon, SearchPill, useCompactColors,
} from '@lib/tamagui-design-system';
import {
  Building2, ChevronDown, Lightbulb, MessageSquare, PenLine, Play, Share, Star, Tag, ThumbsDown, ThumbsUp,
} from '@lib/tamagui-design-system/icons';
import { router, type Href } from 'expo-router';
import { useEffect, useMemo, useState } from 'react';
import { Keyboard, Platform, Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import type { Solution } from '../../domain/library/library';
import type { Blind75Problem } from '../../domain/models';
import { useLibrary } from '../../state/library/useLibrary';
import { useLeetCodePorts } from '../../state/ports';
import { NeetCodeLesson } from './NeetCodeLesson';
import { shareProblem } from './libraryShare';
import { ProblemInformationSheet, type ProblemInformation } from './ProblemInformationSheets';

const tabs = [
  { id: 'description', label: 'Description' },
  { id: 'submissions', label: 'Submissions' },
  { id: 'editorial', label: 'Editorial ▪', accent: true },
  { id: 'solutions', label: 'Solutions' },
] as const;

const filters = [
  { id: 'all', label: 'All' }, { id: 'mine', label: 'My Solution' },
  { id: 'Java', label: 'Java' }, { id: 'Python3', label: 'Python3' }, { id: 'Editorial', label: 'Editorial' },
] as const;

export function ProblemDetailScreen({ id }: { id: string }) {
  const colors = useCompactColors();
  const { top, bottom } = useSafeAreaInsets();
  const { libraryContent, editorBridge } = useLeetCodePorts();
  const problem = libraryContent.problem(id);
  const row = libraryContent.rows.find((entry) => entry.id === id);
  const { lists, actions } = useLibrary();
  const favorited = lists.some((list) => list.id === 'favorite' && list.problemIds.includes(id));
  const [tab, setTab] = useState<(typeof tabs)[number]['id']>('description');
  const [information, setInformation] = useState<ProblemInformation>();
  const [scrolled, setScrolled] = useState(false);
  const [vote, setVote] = useState<'up' | 'down'>();
  const [discussions, setDiscussions] = useState(false);
  const title = problem ? `${problem.leetcodeId}. ${problem.title}` : row ? `${row.number}. ${row.title}` : 'Problem';
  const submissions = libraryContent.submissions(id);
  const solutions = libraryContent.solutions(id);
  const hints = problem ? libraryContent.hints(problem) : [];
  const study = libraryContent.studyPlan.find((entry) => entry.id === id);
  const lesson = useMemo(() => tab === 'editorial' && study ? libraryContent.lesson(id) : undefined, [id, libraryContent, study, tab]);
  const editorRoute = useMemo(() => ({ pathname: '/leetcode/[id]' as const, params: { id, lang: 'javascript' } }), [id]);
  const hasProblem = Boolean(problem);
  useEffect(() => {
    if (hasProblem) void actions.openProblem(id);
  }, [actions, hasProblem, id]);
  useEffect(() => {
    if (Platform.OS !== 'ios' || !hasProblem) return;
    const work = requestIdleCallback(() => editorBridge.pageSource?.(editorBridge.html()));
    return () => cancelIdleCallback(work);
  }, [editorBridge, hasProblem]);
  const openEditor = () => router.push(editorRoute);

  return (
    <View testID="AlgorithmsProblemDetail" style={[styles.screen, { backgroundColor: colors.background, paddingTop: top }]}>
      <CompactHeader title={scrolled ? title : undefined} closeIcon={ChevronDown} closeLabel="Close problem"
        onClose={() => router.canGoBack() ? router.back() : router.replace('/')} trailing={
          <View style={styles.headerActions}>
            <CompactIconButton icon={Star} label={favorited ? 'Remove favorite' : 'Add favorite'} testID="ProblemFavorite"
              color={favorited ? colors.gold : undefined} filled={favorited} onPress={() => { void actions.favorite(id); }} />
            <CompactIconButton icon={Share} label="Share problem" testID="ProblemShare" onPress={() => { void shareProblem(id, title); }} />
          </View>
        } />
      <CompactTabs items={tabs} value={tab} onChange={(next) => { setScrolled(false); setTab(next); }} testID="ProblemTab" />
      {tab === 'description' && (
        <>
          <ScrollView testID="ProblemDescriptionContent" showsVerticalScrollIndicator={false} scrollEventThrottle={32}
            onScroll={({ nativeEvent }) => setScrolled(nativeEvent.contentOffset.y > 45)}
            contentContainerStyle={[styles.content, { paddingBottom: bottom + 89 }]}>
            <Text accessibilityRole="header" style={[styles.title, { color: colors.foreground }]}>{title}</Text>
            {study && <Pressable accessibilityRole="button" testID="ProblemOpenLesson" onPress={() => setTab('editorial')}
              style={[styles.lessonLink, { backgroundColor: colors.surface }]}>
              <Text style={{ color: colors.green, fontWeight: '600' }}>NeetCode 150 · {study.group}</Text>
              <Text style={{ color: colors.foreground }}>View lesson ›</Text>
            </Pressable>}
            {problem ? <Description problem={problem} onInformation={setInformation} /> :
              <Text style={{ color: colors.muted }}>The description for this problem isn’t available in the local collection.</Text>}
          </ScrollView>
          {problem && <FloatingBar bottom={bottom + 7}>
            <FloatingSurface style={styles.reactions}>
              <Reaction icon={ThumbsUp} label="Like" value="21.3K" active={vote === 'up'} onPress={() => setVote(vote === 'up' ? undefined : 'up')} />
              <Reaction icon={ThumbsDown} label="Dislike" active={vote === 'down'} onPress={() => setVote(vote === 'down' ? undefined : 'down')} />
              <Reaction icon={MessageSquare} label="Discussion" value="2.1K" active={discussions} onPress={() => setDiscussions(!discussions)} />
            </FloatingSurface>
            <CompactIconButton icon={Play} label="Open editor" testID="ProblemOpenEditor" emphasis filled onPress={openEditor} />
          </FloatingBar>}
        </>
      )}
      {tab === 'submissions' && (
        <ScrollView testID="ProblemSubmissions" contentContainerStyle={[styles.content, { paddingTop: 0, paddingBottom: bottom }]}>
          {submissions.map((submission) => (
            <DenseActionRow density="comfortable" key={submission.id} testID={`ProblemSubmission-${submission.id}`} label={`${submission.status}, ${submission.date}`}
              onPress={() => router.push(`/leetcode/${id}/submission/${submission.id}` as Href)}
              trailing={<Text style={{ color: colors.muted, fontSize: 13 }}>{submission.date}</Text>}>
              <View style={styles.inline}>
                <Text style={{ color: submission.status === 'Accepted' ? colors.green : colors.red, fontWeight: '600' }}>{submission.status}</Text>
                <Text style={[styles.language, { color: colors.muted, backgroundColor: colors.raised }]}>{submission.language}</Text>
              </View>
              <Text style={{ color: colors.muted, fontSize: 12 }}>◷ {submission.runtime ?? 'N/A'}   ▧ {submission.memory ?? 'N/A'}</Text>
            </DenseActionRow>
          ))}
          {!submissions.length && <Empty text="No submissions yet." />}
        </ScrollView>
      )}
      {tab === 'editorial' && lesson && <NeetCodeLesson title={problem?.title ?? title} lesson={lesson} />}
      {tab === 'editorial' && !lesson && (
        <ScrollView contentContainerStyle={styles.content}>
          {solutions[0] ? <>
            <Text style={[styles.title, { color: colors.foreground }]}>{solutions[0].title}</Text>
            <Text style={{ color: colors.foreground, lineHeight: 20 }}>{solutions[0].summary}</Text>
          </> : <Empty text="No editorial is available in the local collection." />}
        </ScrollView>
      )}
      {tab === 'solutions' && <Solutions solutions={solutions} />}
      {information && problem && <ProblemInformationSheet kind={information} topics={problem.topics} companies={libraryContent.companies} hints={hints} onClose={() => setInformation(undefined)} />}
      {discussions && <CompactMessage title="Discussion" onClose={() => setDiscussions(false)} />}
    </View>
  );
}

function Description({ problem, onInformation }: { problem: Blind75Problem; onInformation: (value: ProblemInformation) => void }) {
  const colors = useCompactColors();
  const difficultyColor = problem.difficulty === 'easy' ? colors.green : problem.difficulty === 'medium' ? colors.gold : colors.red;
  const examples = problem.examples.split(/(```[\s\S]*?```)/).filter((part) => part.trim());
  return (
    <>
      <View style={styles.chips}>
        <Text style={[styles.chip, { color: difficultyColor, backgroundColor: `${difficultyColor}23` }]}>{problem.difficulty[0].toUpperCase() + problem.difficulty.slice(1)}</Text>
        {([
          ['topics', 'Topics', Tag], ['companies', 'Companies', Building2], ['hints', 'Hints', Lightbulb],
        ] as const).map(([kind, label, icon]) => (
          <Pressable key={kind} accessibilityRole="button" testID={`ProblemInfoOpen-${kind}`} onPress={() => onInformation(kind)}
            style={[styles.chipButton, { backgroundColor: colors.surface }]}>
            <Icon as={icon} size={12} color={kind === 'companies' ? colors.gold : colors.foreground} />
            <Text style={{ color: kind === 'companies' ? colors.gold : colors.foreground, fontSize: 12 }}>{label}</Text>
          </Pressable>
        ))}
      </View>
      <Text style={[styles.paragraph, { color: colors.foreground }]}>{problem.id === 'two-sum' ? <>
        You are given an array of integers <InlineCode>nums</InlineCode> and an integer <InlineCode>target</InlineCode>, return <Text style={styles.italic}>indices of the two numbers such that they add up to <InlineCode>target</InlineCode>.</Text>
        {'\n\n'}You may assume that each input would have <Text style={styles.bold}>exactly one solution</Text>, and you may not use the <Text style={styles.italic}>same</Text> element twice.
        {'\n\n'}You can return the answer in any order.
      </> : problem.description}</Text>
      <View style={styles.examples}>
        {examples.map((part, index) => part.startsWith('```') ? (
          problem.id === 'two-sum' ? <TwoSumExample key={index} text={part.slice(3, -3).trim()} index={Math.floor(index / 2)} /> :
            <Text key={index} style={[styles.code, { color: colors.foreground, borderColor: colors.raised }]}>{part.slice(3, -3).trim()}</Text>
        ) : (
          <Text key={index} style={[styles.exampleTitle, { color: colors.foreground }]}>{part.trim().replaceAll('**', '')}</Text>
        ))}
      </View>
      {!!problem.constraints && <>
        <Text style={[styles.exampleTitle, { color: colors.foreground }]}>Constraints:</Text>
        <Text style={[styles.paragraph, { color: colors.foreground }]}>{problem.constraints.replace(/^- /gm, '•  ')}</Text>
      </>}
      {problem.id === 'two-sum' && <Text style={[styles.paragraph, { color: colors.foreground }]}><Text style={{ fontWeight: '700' }}>Follow-up:</Text> Can you come up with an algorithm that is less than O(n²) time complexity?</Text>}
    </>
  );
}

function InlineCode({ children }: { children: string }) {
  const colors = useCompactColors();
  return <Text style={{ fontFamily: 'monospace', fontSize: 13, backgroundColor: colors.surface }}>{children}</Text>;
}

const twoSumExplanation = [
  ['nums[0]', 'nums[1]', '2 + 7 = 9', '[0,1]'],
  ['nums[1]', 'nums[2]', '2 + 4 = 6', '[1,2]'],
  ['nums[0]', 'nums[1]', '3 + 3 = 6', '[0,1]'],
];

function TwoSumExample({ text, index }: { text: string; index: number }) {
  const colors = useCompactColors();
  const [input, output] = text.split('\n');
  const values = twoSumExplanation[index];
  return (
    <View style={styles.exampleBody}>
      <Text style={[styles.exampleLine, { color: colors.foreground }]}>
        <Text style={styles.bold}>Input: </Text><InlineCode>{input.replace(/^Input: /, '')}</InlineCode>
      </Text>
      <Text style={[styles.exampleLine, { color: colors.foreground }]}>
        <Text style={styles.bold}>Output: </Text><InlineCode>{output.replace(/^Output: /, '')}</InlineCode>
      </Text>
      <Text style={[styles.exampleLine, styles.bold, { color: colors.foreground }]}>Explanation:</Text>
      {values && <Text style={[styles.exampleLine, { color: colors.foreground }]}>
        The sum of <InlineCode>{values[0]}</InlineCode> and <InlineCode>{values[1]}</InlineCode> is <InlineCode>{values[2]}</InlineCode>, which equals the target. Therefore, the indices <InlineCode>{values[3]}</InlineCode> are returned.
      </Text>}
    </View>
  );
}

function Reaction({ icon, label, value, active, onPress }: {
  icon: typeof ThumbsUp; label: string; value?: string; active?: boolean; onPress: () => void;
}) {
  const colors = useCompactColors();
  const color = active ? colors.green : colors.muted;
  return (
    <Pressable accessibilityRole="button" accessibilityLabel={label} onPress={onPress} style={styles.reaction}>
      <Icon as={icon} size={13} color={color} />
      {value && <Text style={{ color, fontSize: 12 }}>{value}</Text>}
    </Pressable>
  );
}

function Empty({ text }: { text: string }) {
  const colors = useCompactColors();
  return <Text style={[styles.empty, { color: colors.muted }]}>{text}</Text>;
}

function CompactMessage({ title, onClose }: { title: string; onClose: () => void }) {
  const colors = useCompactColors();
  return (
    <View style={[styles.message, { backgroundColor: colors.surface }]}>
      <CompactHeader title={title} closeIcon={ChevronDown} onClose={onClose} />
      <Empty text="There are no local discussions for this problem." />
    </View>
  );
}

function Solutions({ solutions }: { solutions: ReadonlyArray<Solution> }) {
  const colors = useCompactColors();
  const { bottom } = useSafeAreaInsets();
  const [filter, setFilter] = useState<(typeof filters)[number]['id']>('all');
  const [query, setQuery] = useState('');
  const [selected, setSelected] = useState<Solution>();
  const [composing, setComposing] = useState(false);
  const [draft, setDraft] = useState('');
  const [local, setLocal] = useState<Solution[]>([]);
  const filtered = [...local, ...solutions].filter((solution) =>
    (filter === 'all' || (filter === 'mine' ? solution.author === 'You' : solution.language === filter))
      && `${solution.title} ${solution.summary}`.toLowerCase().includes(query.trim().toLowerCase())
  );
  if (selected || composing) {
    return <View style={styles.screen}>
      <CompactHeader title={selected?.title || 'New solution'} onClose={() => { setSelected(undefined); setComposing(false); }} />
      {selected ? <Text style={[styles.paragraph, styles.content, { color: colors.foreground }]}>{selected.summary}</Text> :
        <View style={styles.content}>
          <SearchPill placeholder="Solution title" value={draft} onChangeText={setDraft} testID="ProblemSolutionDraft" />
          <Pressable accessibilityRole="button" disabled={!draft.trim()} onPress={() => {
            setLocal([{ id: `local-${local.length}`, author: 'You', title: draft.trim(), summary: '', language: 'Java', metrics: '' }, ...local]);
            setDraft('');
            setComposing(false);
            setFilter('mine');
          }}><Text style={[styles.paragraph, { color: colors.green }]}>Save solution</Text></Pressable>
        </View>}
    </View>;
  }
  return (
    <View style={styles.screen}>
      <CompactTabs items={filters} value={filter} onChange={setFilter} testID="ProblemSolutionFilter" />
      <ScrollView testID="ProblemSolutions" contentContainerStyle={[styles.content, { paddingBottom: bottom + 114 }]}>
        {filtered.map((solution) => (
          <DenseActionRow key={solution.id} onPress={() => setSelected(solution)} label={solution.title} testID={`ProblemSolution-${solution.id}`}>
            <Text style={{ color: colors.muted, fontSize: 12 }}>{solution.author}</Text>
            <Text style={{ color: colors.foreground, fontSize: 16, fontWeight: '600' }}>{solution.title}</Text>
            <Text style={{ color: colors.muted, fontSize: 13 }} numberOfLines={2}>{solution.summary}</Text>
            <Text style={{ color: colors.muted, fontSize: 11 }}>{solution.metrics}</Text>
          </DenseActionRow>
        ))}
        {!filtered.length && <Empty text="No solutions match these filters." />}
      </ScrollView>
      <FloatingBar bottom={bottom + 55} style={{ justifyContent: 'flex-end' }}>
        <CompactIconButton icon={PenLine} label="Compose solution" testID="ProblemComposeSolution" emphasis onPress={() => setComposing(true)} />
      </FloatingBar>
      <FloatingBar bottom={bottom + 5}>
        <View style={{ flex: 1 }}><SearchPill value={query} onChangeText={setQuery} onSubmit={Keyboard.dismiss} testID="ProblemSolutionSearch" /></View>
      </FloatingBar>
    </View>
  );
}

const styles = StyleSheet.create({
  screen: { flex: 1 },
  lessonLink: { marginBottom: 12, padding: 12, borderRadius: 12, flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', gap: 8, flexWrap: 'wrap' },
  headerActions: { flexDirection: 'row', gap: 2 },
  content: { paddingHorizontal: 18, paddingTop: 14 },
  title: { fontSize: 22, fontWeight: '600', marginBottom: 12 },
  chips: { flexDirection: 'row', alignItems: 'center', gap: 6, flexWrap: 'wrap' },
  chip: { fontSize: 12, borderRadius: 12, overflow: 'hidden', paddingHorizontal: 8, paddingVertical: 4 },
  chipButton: { flexDirection: 'row', alignItems: 'center', gap: 4, borderRadius: 12, paddingHorizontal: 8, minHeight: 23 },
  paragraph: { fontSize: 16, lineHeight: 20, marginTop: 14 },
  bold: { fontWeight: '700' },
  italic: { fontStyle: 'italic' },
  exampleBody: { gap: 9 },
  exampleLine: { fontSize: 16, lineHeight: 19 },
  examples: { marginTop: 25, marginBottom: 12 },
  exampleTitle: { fontSize: 15, fontWeight: '700', marginVertical: 9 },
  code: { fontFamily: 'monospace', fontSize: 13, lineHeight: 18, paddingLeft: 11, borderLeftWidth: 2 },
  inline: { flexDirection: 'row', alignItems: 'center', gap: 9 },
  language: { fontSize: 12, paddingHorizontal: 7, paddingVertical: 2, borderRadius: 11, overflow: 'hidden' },
  reactions: { flexDirection: 'row', paddingHorizontal: 7, minHeight: 44, alignItems: 'center' },
  reaction: { paddingHorizontal: 6, minHeight: 32, flexDirection: 'row', alignItems: 'center', gap: 4 },
  empty: { marginVertical: 24, textAlign: 'center' },
  message: { position: 'absolute', bottom: 0, left: 0, right: 0, minHeight: 200, borderTopLeftRadius: 28, borderTopRightRadius: 28 },
});
