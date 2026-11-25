import { FloatingSurface, useCompactColors } from '@lib/tamagui-design-system';
import { Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';

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
      <Text accessibilityRole="header" style={[styles.title, foreground]}>Task {task.number} · Flash cards</Text>
      <View style={styles.directions}>
        {(['english-first', 'tamil-first'] as const).map((direction) => {
          const selected = session.direction === direction;
          return (
            <Pressable key={direction} accessibilityRole="radio" accessibilityState={{ checked: selected }}
              disabled={pending} testID={`TamilFlash-${direction}`}
              style={[styles.direction, { backgroundColor: colors.surface, borderColor: selected ? colors.green : colors.raised }]}
              onPress={() => { if (!selected) void practice.load(direction); }}>
              <Text style={{ color: selected ? colors.green : colors.foreground }}>
                {direction === 'english-first' ? 'English → Tamil' : 'Tamil → English'}
              </Text>
            </Pressable>
          );
        })}
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
          {(!englishFirst || revealed) && <CourseButton label="Play Tamil" testID="TamilFlashAudio" accessibilityLabel={practice.played ? 'Play Tamil, recording played' : undefined}
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
  direction: { minHeight: 44, paddingHorizontal: 14, borderWidth: 1, borderRadius: 22, justifyContent: 'center' },
  card: { gap: 20, padding: 20, borderRadius: 18 },
  phrase: { fontSize: 22, fontWeight: '600' },
  answer: { gap: 6 },
});
