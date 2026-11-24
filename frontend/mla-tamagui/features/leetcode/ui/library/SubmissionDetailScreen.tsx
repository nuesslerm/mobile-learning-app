import { Logger } from '@lib/logging';
import { BarChart, CodeBlock, CompactHeader, CompactIconButton, LineChart, useCompactColors } from '@lib/tamagui-design-system';
import { Share } from '@lib/tamagui-design-system/icons';
import { router } from 'expo-router';
import { useEffect, useState } from 'react';
import { Platform, Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import type { Submission } from '../../domain/library/library';
import { useLeetCodePorts } from '../../state/ports';
import { shareProblem } from './libraryShare';

type LoadState = { status: 'loading' | 'empty' | 'error' } | { status: 'loaded'; submission: Submission };

const runtimeBars = [{ label: '27 ms', value: 100 }, { label: '593 ms', value: 1 }, { label: '1168 ms', value: 2 },
  { label: '1743 ms', value: 8 }, { label: '2318 ms', value: 1 }];
const memoryBars = [{ label: '19.7 MB', value: 36 }, { label: '20.3 MB', value: 92 }, { label: '20.5 MB', value: 72 },
  { label: '21 MB', value: 24 }, { label: '22 MB', value: 9 }];
const complexity = [{ value: 0 }, { value: 1 }, { value: 2 }, { value: 3 }, { value: 4 }];
const keywords = /\b(class|def|for|in|if|return)\b/g;

export function SubmissionDetailScreen({ id, submissionId }: { id: string; submissionId: string }) {
  const colors = useCompactColors();
  const { libraryContent } = useLeetCodePorts();
  const { top, bottom } = useSafeAreaInsets();
  const [state, setState] = useState<LoadState>({ status: 'loading' });
  const [attempt, setAttempt] = useState(0);
  const [metric, setMetric] = useState<'Runtime' | 'Memory'>('Runtime');
  useEffect(() => {
    let active = true;
    setState({ status: 'loading' });
    libraryContent.submission(id, submissionId).then((submission) => {
      if (active) setState(submission ? { status: 'loaded', submission } : { status: 'empty' });
    }).catch((error: unknown) => {
      Logger.withError(error).error('leetcode.library.submission_failed');
      if (active) setState({ status: 'error' });
    });
    return () => { active = false; };
  }, [attempt, id, libraryContent, submissionId]);

  const problem = libraryContent.problem(id);
  const title = problem ? `${problem.leetcodeId}. ${problem.title}` : 'Problem';
  return (
    <View
      testID="ProblemSubmissionDetail"
      style={[styles.screen, { backgroundColor: colors.background, paddingTop: Platform.OS === 'ios' ? 0 : top }]}
    >
      <CompactHeader title="Submission" onClose={() => router.canGoBack() ? router.back() : router.replace('/')}
        trailing={<CompactIconButton icon={Share} label="Share problem" testID="ProblemSubmissionShare" onPress={() => { void shareProblem(id, title); }} />} />
      <ScrollView contentContainerStyle={[styles.content, { paddingBottom: bottom + 12 }]}>
        {state.status === 'loading' && <View accessibilityLabel="Loading submission" testID="ProblemSubmissionLoading" style={{ gap: 14 }}>
          {[180, 140, 120, 155].map((width) => <View key={width} style={{ width, height: 13, borderRadius: 5, backgroundColor: colors.raised }} />)}
          <View style={{ height: 260, borderRadius: 15, backgroundColor: colors.surface }} />
        </View>}
        {state.status === 'empty' && <Text style={{ color: colors.muted }}>This submission is unavailable.</Text>}
        {state.status === 'error' && <>
          <Text style={{ color: colors.muted }}>Couldn’t load the submission.</Text>
          <Pressable accessibilityRole="button" onPress={() => setAttempt(attempt + 1)}><Text style={{ color: colors.green }}>Retry</Text></Pressable>
        </>}
        {state.status === 'loaded' && <>
          <View style={{ gap: 4 }}>
            <Text accessibilityRole="header" testID="ProblemSubmissionTitle" style={[styles.title, { color: colors.foreground }]}>{title}</Text>
            <Text style={{ color: colors.muted, fontSize: 13 }}>Submitted on {state.submission.date}</Text>
          </View>
          <View>
            <Metric label={state.submission.status} value={state.submission.status === 'Accepted' ? '66 / 66 testcases passed' : 'Execution failed'}
              glyph={state.submission.status === 'Accepted' ? '✓' : '!'} tint={state.submission.status === 'Accepted' ? colors.green : colors.red} />
            <Metric label={`Runtime ${state.submission.runtime ?? 'N/A'}`} value={state.submission.runtime ? 'Beats 100.00%' : ''} glyph="◷" tint="#69adff" onPress={() => setMetric('Runtime')} />
            <Metric label={`Memory ${state.submission.memory ?? 'N/A'}`} value={state.submission.memory ? 'Beats 12.41%' : ''} glyph="▤" tint="#c48af1" onPress={() => setMetric('Memory')} />
          </View>
          {state.submission.runtime && <View style={[styles.distribution, { backgroundColor: colors.background === '#000' ? '#191624' : colors.surface }]} testID="ProblemSubmissionDistribution">
            <View style={styles.cardHeader}>
              <Text style={{ color: colors.foreground, fontWeight: '600' }}>{metric} Distribution</Text>
              <Text style={{ color: colors.muted }}>{metric === 'Runtime' ? state.submission.runtime : state.submission.memory}</Text>
            </View>
            <BarChart variant="distribution" data={metric === 'Runtime' ? runtimeBars : memoryBars} height={174} />
            <View style={[styles.cardHeader, { marginTop: 12 }]}>
              <View style={{ flex: 1, gap: 4 }}>
                <Text style={{ color: colors.foreground, fontWeight: '600' }}>Time Complexity</Text>
                <Text style={{ color: colors.muted }}>Current: O(N)</Text>
                <Text style={{ color: '#b77de8' }}>Suggested: O(N)</Text>
              </View>
              <View style={{ width: 128 }}><LineChart data={complexity} height={77} color="accent" /></View>
            </View>
            <Text style={{ color: colors.muted, lineHeight: 21, fontSize: 16 }}>You have already reached the optimal time complexity with your hash table implementation.</Text>
            <Text style={{ color: colors.muted, textAlign: 'center', fontSize: 12 }}>•  ·</Text>
          </View>}
          {state.submission.status === 'Accepted' && <CodeBlock title="Code" language={state.submission.language} code={libraryContent.submissionCode}
            testID="ProblemSubmissionCode" surfaceColor={colors.background === '#000' ? '#131416' : colors.surface} renderCode={(code) => code.split(keywords).map((token, index) =>
              index % 2 ? <Text key={index} style={{ color: '#f178af' }}>{token}</Text> : token
            )} />}
        </>}
      </ScrollView>
    </View>
  );
}

function Metric({ label, value, glyph, tint, onPress }: { label: string; value: string; glyph: string; tint: string; onPress?: () => void }) {
  const colors = useCompactColors();
  const content = <>
    <View style={[styles.glyph, { backgroundColor: `${tint}25` }]}><Text style={{ color: tint, fontSize: 16 }}>{glyph}</Text></View>
    <Text style={{ color: colors.foreground, fontSize: 15, fontWeight: '500', flex: 1 }}>{label}</Text>
    <Text style={{ color: colors.muted, fontSize: 13 }}>{value}</Text>
  </>;
  return onPress ? <Pressable style={styles.metric} accessibilityRole="button" accessibilityLabel={label} onPress={onPress}>{content}</Pressable>
    : <View style={styles.metric}>{content}</View>;
}

const styles = StyleSheet.create({
  screen: { flex: 1 },
  content: { paddingHorizontal: 18, paddingTop: 24, gap: 19 },
  title: { fontSize: 21, fontWeight: '600' },
  metric: { flexDirection: 'row', alignItems: 'center', gap: 10, minHeight: 41 },
  glyph: { width: 31, height: 31, borderRadius: 16, justifyContent: 'center', alignItems: 'center' },
  distribution: { borderRadius: 13, padding: 15, gap: 16 },
  cardHeader: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', gap: 8 },
});
