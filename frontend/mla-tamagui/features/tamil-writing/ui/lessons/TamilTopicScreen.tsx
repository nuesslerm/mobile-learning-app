import { DenseActionRow, Icon, SearchPill, useCompactColors } from '@lib/tamagui-design-system';
import { BookOpen, ChevronRight } from '@lib/tamagui-design-system/icons';
import { router, useFocusEffect } from 'expo-router';
import { useCallback, useState } from 'react';
import { Keyboard, ScrollView, StyleSheet, Text, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { matchesTask, nextTask, reviewExercises } from '../../domain/lessons/course';
import { useCourse } from '../../state/lessons/useCourse';
import { backTo, CourseButton, CourseLayout, CourseMessage } from '../library/CourseLayout';

export function TamilTopicScreen({ topicId }: { topicId: string }) {
  const colors = useCompactColors();
  const { bottom } = useSafeAreaInsets();
  const { course, error, reload } = useCourse();
  const [query, setQuery] = useState('');
  useFocusEffect(useCallback(() => { void reload(); }, [reload]));
  const topic = course?.sections.flatMap((section) => section.topics).find((item) => item.id === topicId);
  const progress = course?.progress;
  const completed = topic?.tasks.filter((task) => progress?.tasks[task.id]?.completed).length ?? 0;
  const next = topic && progress ? nextTask(topic.tasks, progress.tasks, progress.active[topic.id]) : undefined;
  const review = topic?.tasks.find((task) => reviewExercises(task, progress?.tasks[task.id]).length > 0);
  const filtered = topic?.tasks.filter((task) => matchesTask(task, query)) ?? [];
  function openTask(id: string) {
    Keyboard.dismiss();
    router.push({ pathname: '/tamil/lessons/[groupId]/[taskId]', params: { groupId: topicId, taskId: id } });
  }

  return (
    <CourseLayout title={topic?.title ?? 'Tamil Lessons'} onBack={() => backTo('/tamil')} testID="TamilTopic">
      {!topic ? <CourseMessage text={error ? 'Couldn’t load this topic.' : course ? 'Topic unavailable.' : 'Loading lessons…'}
        retry={error ? () => { void reload(); } : undefined} /> : (
        <>
          <View style={styles.search}>
            <SearchPill value={query} onChangeText={setQuery} onSubmit={Keyboard.dismiss}
              onCancel={query ? () => { setQuery(''); Keyboard.dismiss(); } : undefined} testID="TamilTaskSearch" />
          </View>
          <ScrollView keyboardShouldPersistTaps="handled" contentContainerStyle={[styles.content, { paddingBottom: bottom + 24 }]}>
            <View style={styles.hero}>
              <View style={[styles.artwork, { backgroundColor: colors.surface }]}>
                <Icon as={BookOpen} color={colors.gold} size={34} />
              </View>
              <Text accessibilityRole="header" style={[styles.title, { color: colors.foreground }]}>{topic.title}</Text>
              <Text style={{ color: colors.muted }}>{completed}/{topic.tasks.length} tasks completed</Text>
              <View accessibilityRole="progressbar" accessibilityValue={{ min: 0, max: topic.tasks.length, now: completed }}
                style={[styles.track, { backgroundColor: colors.raised }]}>
                <View style={{ backgroundColor: colors.green, height: 4, width: `${100 * completed / topic.tasks.length}%` }} />
              </View>
              <CourseButton label={next ? 'Practice' : 'Review'} testID="TamilPractice"
                onPress={() => { const task = next ?? review ?? topic.tasks[0]; if (task) openTask(task.id); }} />
            </View>
            {filtered.map((task) => {
              const tint = task.difficulty === 'Easy' ? colors.green : task.difficulty === 'Medium' ? colors.gold : colors.red;
              const finished = progress?.tasks[task.id]?.completed;
              return (
                <DenseActionRow key={task.id} label={`${task.number}. ${task.title}`} onPress={() => openTask(task.id)}
                  testID={`TamilTask-${task.id}`} trailing={<Icon as={ChevronRight} size={17} color={colors.muted} />}>
                  <View style={styles.row}>
                    <Text style={[styles.badge, { color: tint, backgroundColor: `${tint}23` }]}>{task.difficulty}</Text>
                    <Text numberOfLines={1} style={[styles.rowTitle, { color: colors.foreground }]}>{task.number}. {task.title}</Text>
                  </View>
                  <Text style={{ color: colors.muted, fontSize: 12 }}>{finished ? '✓  Completed' : `${task.exercises.length} exercises`}</Text>
                </DenseActionRow>
              );
            })}
            {!filtered.length && <CourseMessage text="No tasks match your search." />}
          </ScrollView>
        </>
      )}
    </CourseLayout>
  );
}

const styles = StyleSheet.create({
  search: { paddingHorizontal: 18, paddingVertical: 4 },
  content: { paddingHorizontal: 18 },
  hero: { alignItems: 'center', paddingVertical: 16, gap: 9 },
  artwork: { width: 72, height: 72, borderRadius: 14, alignItems: 'center', justifyContent: 'center' },
  title: { fontSize: 24, fontWeight: '700' },
  track: { width: 160, height: 4, borderRadius: 2, overflow: 'hidden' },
  row: { flexDirection: 'row', alignItems: 'center', gap: 8 },
  badge: { borderRadius: 4, paddingHorizontal: 5, paddingVertical: 2, fontSize: 11, fontWeight: '700' },
  rowTitle: { flex: 1, fontSize: 15, fontWeight: '600' },
});
