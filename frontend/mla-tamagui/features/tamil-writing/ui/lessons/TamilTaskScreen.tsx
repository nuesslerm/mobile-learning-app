import { FloatingSurface, useCompactColors } from '@lib/tamagui-design-system';
import { useRef, useState, type ComponentRef } from 'react';
import { Keyboard, Pressable, ScrollView, StyleSheet, Text, TextInput, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import type { CourseProgress, Exercise, Task, Topic } from '../../domain/lessons/course';
import { tilesFor, type Tile } from '../../domain/lessons/tiles';
import { useCourse } from '../../state/lessons/useCourse';
import { useTaskPractice } from '../../state/lessons/useTaskPractice';
import { backTo, CourseButton, CourseLayout, CourseMessage } from '../library/CourseLayout';

const titles = { translate: 'Translate into Tamil', fill: 'Complete the sentence', listen: 'Tap what you hear' };

export function TamilTaskScreen({ topicId, taskId }: { topicId: string; taskId: string }) {
  const { course, error, reload, save } = useCourse();
  const topic = course?.sections.flatMap((section) => section.topics).find((item) => item.id === topicId);
  const task = topic?.tasks.find((item) => item.id === taskId);
  return (
    <CourseLayout title={topic?.title ?? 'Tamil Practice'} testID="TamilPracticeScreen"
      onBack={() => backTo({ pathname: '/tamil/lessons/[groupId]', params: { groupId: topicId } })}>
      {course && topic && task ? <TaskPractice key={task.id} topic={topic} task={task} progress={course.progress} save={save} />
        : <CourseMessage text={error ? 'Couldn’t load this task.' : course ? 'Task unavailable.' : 'Loading practice…'}
          retry={error ? () => { void reload(); } : undefined} />}
    </CourseLayout>
  );
}

function TaskPractice({ topic, task, progress, save }: {
  topic: Topic; task: Task; progress: CourseProgress; save: ReturnType<typeof useCourse>['save'];
}) {
  const colors = useCompactColors();
  const { bottom } = useSafeAreaInsets();
  const practice = useTaskPractice(topic.id, task, progress, save);
  if (practice.preview) {
    const example = task.exercises[0];
    return (
      <>
        <ScrollView contentContainerStyle={styles.body}>
          <Text style={[styles.heading, { color: colors.foreground }]}>{task.title}</Text>
          <Text style={[styles.label, { color: colors.muted }]}>Worked example</Text>
          <FloatingSurface style={styles.card}>
            <Text style={[styles.prompt, { color: colors.foreground }]}>{example.english}</Text>
            <Text style={[styles.prompt, { color: colors.foreground }]} accessibilityLanguage="ta-IN">{example.tamil}</Text>
            <Text style={[styles.label, { color: colors.muted }]}>{example.transliteration}</Text>
          </FloatingSurface>
          <CourseButton label="Play Tamil" onPress={() => { void practice.play(example.id, false); }} />
          {practice.error && <CourseMessage text={practice.error} />}
        </ScrollView>
        <View style={[styles.footer, { paddingBottom: Math.max(bottom, 12) }]}>
          <CourseButton label="Start practice" testID="TamilStartPractice" onPress={practice.begin} />
        </View>
      </>
    );
  }

  if (practice.exercise) return <Prompt key={practice.exercise.id} exercise={practice.exercise} practice={practice} />;

  const answers = Object.values(practice.answers);
  const correct = answers.filter((answer) => answer === 'correct').length;
  const missed = answers.filter((answer) => answer === 'missed').length;
  const skipped = answers.filter((answer) => answer === 'skipped').length;
  return (
    <ScrollView contentContainerStyle={[styles.body, { paddingBottom: bottom + 20 }]} testID="TamilTaskSummary">
      <Text style={[styles.heading, { color: colors.foreground }]}>Task complete</Text>
      <FloatingSurface style={styles.card}>
        <Text style={[styles.prompt, { color: colors.green }]}>{correct} correct</Text>
        <Text style={[styles.label, { color: colors.foreground }]}>{missed} missed · {skipped} skipped</Text>
      </FloatingSurface>
      {missed + skipped > 0 && <CourseButton label="Review missed and skipped" onPress={practice.review} />}
      <CourseButton label="Practice again" onPress={practice.restart} />
      <CourseButton label="Back to tasks" onPress={() => backTo({ pathname: '/tamil/lessons/[groupId]', params: { groupId: topic.id } })} />
    </ScrollView>
  );
}

function Prompt({ exercise, practice }: { exercise: Exercise; practice: ReturnType<typeof useTaskPractice> }) {
  const colors = useCompactColors();
  const { bottom } = useSafeAreaInsets();
  const scroll = useRef<ComponentRef<typeof ScrollView>>(null);
  const [tileMode, setTileMode] = useState(true);
  const [typed, setTyped] = useState<string>();
  const [selected, setSelected] = useState<readonly Tile[]>([]);
  const [tiles] = useState(() => tilesFor(exercise));
  const { feedback, pending } = practice;
  const disabled = Boolean(feedback) || pending;

  function changeMode() {
    if (tileMode && typed === undefined) setTyped(selected.map((tile) => tile.word).join(' '));
    if (!tileMode) Keyboard.dismiss();
    setTileMode(!tileMode);
  }

  function select(tile: Tile) {
    setSelected((previous) => exercise.kind === 'fill' ? [tile] : [...previous, tile]);
  }

  function remove(tile: Tile) {
    setSelected((previous) => previous.filter((item) => item.id !== tile.id));
  }

  return (
    <>
      <ScrollView ref={scroll} testID={`TamilExercise-${exercise.kind}`} keyboardShouldPersistTaps="handled"
        onContentSizeChange={() => { if (feedback) scroll.current?.scrollToEnd({ animated: true }); }}
        contentContainerStyle={styles.body}>
        <View style={styles.progress} accessibilityRole="progressbar"
          accessibilityValue={{ min: 0, now: practice.number, max: practice.total }}>
          <View style={[styles.track, { backgroundColor: colors.raised }]}>
            <View style={{ width: `${100 * (practice.number - 1) / practice.total}%`, height: 5, backgroundColor: colors.green }} />
          </View>
          <Text style={[styles.label, { color: colors.muted }]}>{practice.number}/{practice.total}</Text>
        </View>
        <Text style={[styles.heading, { color: colors.foreground }]}>{titles[exercise.kind]}</Text>
        {exercise.kind === 'listen' && (
          <View style={styles.audio}>
            <CourseButton label="Play Tamil" testID="TamilPlayAudio" onPress={() => { void practice.play(exercise.id, false); }} />
            <CourseButton label="Slow" testID="TamilSlowAudio" onPress={() => { void practice.play(exercise.id, true); }} />
          </View>
        )}
        {exercise.kind === 'translate' && (
          <FloatingSurface style={styles.card}><Text style={[styles.prompt, { color: colors.foreground }]}>{exercise.english}</Text></FloatingSurface>
        )}
        {exercise.kind === 'fill' && <Text style={[styles.label, { color: colors.muted }]}>{exercise.english}</Text>}
        {exercise.kind === 'fill' ? (
          <View style={styles.tiles}>
            {exercise.words.map((word, index) => index === exercise.blank
              ? selected[0] ? <AnswerTile key={index} tile={selected[0]} disabled={disabled} selected onPress={() => remove(selected[0])} />
                : <View key={index} style={[styles.blank, { borderColor: colors.gold }]} accessibilityLabel="Blank" />
              : <Text key={index} style={[styles.prompt, { color: colors.foreground }]} accessibilityLanguage="ta-IN">{word}</Text>)}
          </View>
        ) : tileMode ? (
          <View style={[styles.tiles, styles.answerArea, { borderColor: colors.raised }]} testID="TamilAnswerTiles" collapsable={false}>
            {selected.map((tile) => <AnswerTile key={tile.id} tile={tile} selected disabled={disabled} onPress={() => remove(tile)} />)}
          </View>
        ) : (
          <TextInput accessibilityLabel="Type your answer in Tamil" testID="TamilTypedAnswer"
            placeholder="Type in Tamil" placeholderTextColor={colors.muted} value={typed} editable={!disabled}
            onChangeText={setTyped} autoCorrect={false} multiline style={[styles.input, { color: colors.foreground, backgroundColor: colors.surface }]} />
        )}
        {tileMode && (
          <View style={styles.tiles} testID="TamilWordBank" collapsable={false}>
            {tiles.map((tile) => selected.some((item) => item.id === tile.id)
              ? <View key={tile.id} style={[styles.ghost, { backgroundColor: colors.surface }]}><Text style={styles.hidden}>{tile.word}</Text></View>
              : <AnswerTile key={tile.id} tile={tile} disabled={disabled} onPress={() => select(tile)} />)}
          </View>
        )}
        {exercise.kind === 'translate' && !feedback && (
          <CourseButton label={tileMode ? 'Use keyboard' : 'Use word bank'} testID="TamilSwitchAnswer" disabled={pending} onPress={changeMode} />
        )}
        {exercise.kind === 'listen' && !feedback && (
          <CourseButton label="Can’t listen now" testID="TamilSkipListening" disabled={pending} onPress={() => { void practice.skip(); }} />
        )}
        {feedback && (
          <FloatingSurface style={styles.card} testID="TamilAnswerFeedback">
            <Text accessibilityLiveRegion="polite" style={[styles.prompt, { color: feedback.correct ? colors.green : colors.red }]}>
              {feedback.correct ? 'Correct' : 'Incorrect'}
            </Text>
            <Text style={[styles.prompt, { color: colors.foreground }]} accessibilityLanguage="ta-IN">{exercise.tamil}</Text>
            {!feedback.correct && exercise.kind === 'fill' && <Text style={[styles.label, { color: colors.foreground }]}>Missing word · {feedback.expected}</Text>}
            <Text style={[styles.label, { color: colors.muted }]}>{exercise.transliteration} · {exercise.english}</Text>
            {exercise.kind !== 'listen' && <CourseButton label="Play Tamil" onPress={() => { void practice.play(exercise.id, false); }} />}
          </FloatingSurface>
        )}
        {practice.error && <CourseMessage text={practice.error} />}
      </ScrollView>
      <View style={[styles.footer, { paddingBottom: Math.max(bottom, 12) }]}>
        <CourseButton label={feedback ? 'Continue' : 'Check'} testID={feedback ? 'TamilContinue' : 'TamilCheck'}
          disabled={pending || (!feedback && (tileMode ? selected.length === 0 : !typed?.trim()))}
          onPress={() => {
            Keyboard.dismiss();
            if (feedback) void practice.advance();
            else void practice.check(tileMode ? selected.map((tile) => tile.word) : typed ?? '');
          }} />
      </View>
    </>
  );
}

function AnswerTile({ tile, selected, disabled, onPress }: {
  tile: Tile; selected?: boolean; disabled: boolean; onPress: () => void;
}) {
  const colors = useCompactColors();
  return (
    <Pressable accessibilityRole="button" accessibilityLabel={selected ? `Remove ${tile.word}` : `Add ${tile.word}`}
      testID={`TamilTile-${selected ? 'answer' : 'bank'}-${tile.id}`} disabled={disabled} onPress={onPress}>
      <FloatingSurface tone={selected ? 'subtle' : undefined} style={[styles.tile, { borderColor: selected ? colors.gold : colors.raised }]}>
        <Text style={[styles.tileText, { color: colors.foreground }]} accessibilityLanguage="ta-IN">{tile.word}</Text>
      </FloatingSurface>
    </Pressable>
  );
}

const styles = StyleSheet.create({
  body: { padding: 18, gap: 18, flexGrow: 1 },
  progress: { flexDirection: 'row', alignItems: 'center', gap: 10 },
  track: { flex: 1, height: 5, borderRadius: 3, overflow: 'hidden' },
  heading: { fontSize: 25, fontWeight: '700' },
  prompt: { fontSize: 20, fontWeight: '600' },
  label: { fontSize: 14, lineHeight: 20 },
  card: { borderRadius: 18, padding: 15, gap: 8 },
  audio: { flexDirection: 'row', gap: 10 },
  tiles: { flexDirection: 'row', flexWrap: 'wrap', alignItems: 'center', gap: 8 },
  answerArea: { minHeight: 62, borderBottomWidth: 1, paddingBottom: 8 },
  tile: { minHeight: 44, borderRadius: 11, paddingHorizontal: 12, borderWidth: 1, justifyContent: 'center' },
  tileText: { fontSize: 17 },
  ghost: { minHeight: 44, borderRadius: 11, paddingHorizontal: 12, justifyContent: 'center' },
  hidden: { fontSize: 17, opacity: 0 },
  blank: { width: 54, height: 36, borderBottomWidth: 2 },
  input: { minHeight: 90, borderRadius: 14, padding: 12, fontSize: 18, textAlignVertical: 'top' },
  footer: { paddingHorizontal: 18, paddingTop: 8 },
});
