import { FloatingSurface, useCompactColors } from '@lib/tamagui-design-system';
import { ScrollView, StyleSheet, Text, View } from 'react-native';

import type { Task } from '../../domain/lessons/course';
import { intervalLabel, ratingLabels, ratings } from '../../domain/lessons/flashCards';
import { useFlashCards } from '../../state/lessons/useFlashCards';
import { CourseButton, CourseMessage } from '../library/CourseLayout';

export function FlashCardPractice({ task }: { task: Task }) {
  const colors = useCompactColors();
  const practice = useFlashCards(task);
  const { session, current, queue, options, pending, revealed } = practice;
  if (!session || !queue) return <CourseMessage text={practice.error ?? 'Loading flash cards…'}
    retry={practice.error ? () => { void practice.load(); } : undefined} />;
  const englishFirst = session.direction === 'english-first';
  const foreground = { color: colors.foreground };
  return (
    <ScrollView contentContainerStyle={styles.body} testID="TamilFlashPractice">
      <Text accessibilityRole="header" style={[styles.title, foreground]}>{task.title}</Text>
      <View style={styles.directions}>
        {(['english-first', 'tamil-first'] as const).map((direction) => (
          <CourseButton key={direction} label={direction === 'english-first' ? 'English → Tamil' : 'Tamil → English'}
            disabled={pending || session.direction === direction} testID={`TamilFlash-${direction}`}
            onPress={() => { void practice.load(direction); }} />
        ))}
      </View>
      <Text style={{ color: colors.muted }}>{queue.due} due · {queue.fresh} new</Text>
      {current ? <>
        <FloatingSurface style={styles.card}>
          <Text style={[styles.phrase, foreground]} accessibilityLanguage={englishFirst ? 'en' : 'ta-IN'}>
            {englishFirst ? current.english : current.tamil}
          </Text>
          {revealed && <View style={styles.answer}>
            <Text style={[styles.phrase, foreground]} accessibilityLanguage={englishFirst ? 'ta-IN' : 'en'}>
              {englishFirst ? current.tamil : current.english}
            </Text>
            <Text style={{ color: colors.muted }}>{current.transliteration}</Text>
          </View>}
          {(!englishFirst || revealed) && <CourseButton label="Play Tamil" testID="TamilFlashAudio"
            disabled={pending} onPress={() => { void practice.play(); }} />}
        </FloatingSurface>
        {options ? <View style={styles.directions}>
          {ratings.map((grade) => <CourseButton key={grade} disabled={pending}
            testID={`TamilFlashRate-${ratingLabels[grade]}`}
            label={`${ratingLabels[grade]} · ${intervalLabel(options[grade].card.due.getTime() - session.now)}`}
            onPress={() => { void practice.rate(grade); }} />)}
        </View> : <CourseButton label="Show answer" disabled={pending} testID="TamilFlashReveal" onPress={practice.reveal} />}
      </> : <Text accessibilityLiveRegion="polite" style={foreground} testID="TamilFlashFinished">
        {queue.nextDue ? 'Next review · ' + new Date(queue.nextDue).toLocaleString() : 'No phrases in this task.'}
      </Text>}
      {practice.error && <CourseMessage text={practice.error} />}
    </ScrollView>
  );
}

const styles = StyleSheet.create({
  body: { padding: 18, gap: 16, paddingBottom: 48 },
  title: { fontSize: 24, fontWeight: '700' },
  directions: { flexDirection: 'row', flexWrap: 'wrap', gap: 8 },
  card: { gap: 20, padding: 20, borderRadius: 18 },
  phrase: { fontSize: 22, fontWeight: '600' },
  answer: { gap: 6 },
});
