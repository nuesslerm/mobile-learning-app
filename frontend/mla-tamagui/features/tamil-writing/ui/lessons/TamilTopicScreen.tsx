import { DenseActionRow, Icon, SearchPill, useCompactColors } from '@lib/tamagui-design-system';
import { BookOpen, ChevronRight } from '@lib/tamagui-design-system/icons';
import { router, useFocusEffect } from 'expo-router';
import { useCallback, useState } from 'react';
import { FlatList, Keyboard, StyleSheet, Text, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { exerciseCount, matchesTask, nextTask, reviewExercises } from '../../domain/lessons/course';
import { useCourse } from '../../state/lessons/useCourse';
import { backTo, CourseButton, CourseLayout, CourseMessage } from '../library/CourseLayout';

import { TamilLibraryDock } from '../library/TamilLibraryDock';

export function TamilTopicScreen({ topicId, sectionId }: { topicId?: string; sectionId?: string }) {
  const colors = useCompactColors();
  const { bottom } = useSafeAreaInsets();
  const { course, error, reload } = useCourse();
  const [query, setQuery] = useState('');
  useFocusEffect(useCallback(() => { void reload(); }, [reload]));
  const sections = course?.sections.filter((section) => !sectionId || section.id === sectionId);
  const topics = (sections?.flatMap((section) => section.topics) ?? [])
    .filter((topic) => topicId === undefined || topic.id === topicId);
  const title = topicId ? topics[0]?.title : sectionId ? sections?.[0]?.title : 'All Topics';
  const tasks = topics.flatMap((topic) => topic.tasks.map((task) => ({ topic, task })));
  const progress = course?.progress;
  const completed = tasks.filter(({ task }) => progress?.tasks[task.id]?.completed).length;
  const next = topics.map((topic) => ({
    topic, task: progress && nextTask(topic.tasks, progress.tasks, progress.active[topic.id]),
  })).find((row) => row.task);
  const review = tasks.find(({ task }) => reviewExercises(task, progress?.tasks[task.id]).length > 0) ?? tasks[0];
  const practice = next?.task ? { topic: next.topic, task: next.task } : review;
  const filtered = tasks.filter(({ task }) => matchesTask(task, query));
  function openTask(groupId: string, id: string) {
    Keyboard.dismiss();
    router.push({ pathname: '/tamil/lessons/[groupId]/[taskId]', params: { groupId, taskId: id } });
  }

  return (
    <CourseLayout title={title ?? 'Tamil Lessons'} onBack={() => backTo('/tamil')} testID="TamilTopic">
      {!topics.length ? <CourseMessage text={error ? 'Couldn’t load this topic.' : course ? 'Topic unavailable.' : 'Loading lessons…'}
        retry={error ? () => { void reload(); } : undefined} /> : (
        <>
          <View style={styles.search}>
            <SearchPill value={query} onChangeText={setQuery} onSubmit={Keyboard.dismiss}
              onCancel={query ? () => { setQuery(''); Keyboard.dismiss(); } : undefined} testID="TamilTaskSearch" />
          </View>
          <FlatList data={filtered} keyExtractor={({ task }) => task.id} keyboardShouldPersistTaps="handled" contentContainerStyle={[styles.content, { paddingBottom: bottom + 100 }]}
            ListHeaderComponent={(
            <View style={styles.hero}>
              <View style={[styles.artwork, { backgroundColor: colors.surface }]}>
                <Icon as={BookOpen} color={colors.gold} size={34} />
              </View>
              <Text accessibilityRole="header" style={[styles.title, { color: colors.foreground }]}>{title}</Text>
              <Text style={{ color: colors.muted }}>{completed}/{tasks.length} tasks completed · {exerciseCount(topics).toLocaleString('en-US')} exercises</Text>
              <View accessibilityRole="progressbar" accessibilityValue={{ min: 0, max: tasks.length, now: completed }}
                style={[styles.track, { backgroundColor: colors.raised }]}>
                <View style={{ backgroundColor: colors.green, height: 4, width: `${100 * completed / Math.max(tasks.length, 1)}%` }} />
              </View>
              <CourseButton label={next ? 'Practice' : 'Review'} testID="TamilPractice"
                onPress={() => { if (practice) openTask(practice.topic.id, practice.task.id); }} />
            </View>
            )}
            ListEmptyComponent={<CourseMessage text="No tasks match your search." />}
            renderItem={({ item: { topic, task } }) => {
              const tint = task.difficulty === 'Easy' ? colors.green : task.difficulty === 'Medium' ? colors.gold : colors.red;
              const finished = progress?.tasks[task.id]?.completed;
              return (
                <View><DenseActionRow label={`${task.number}. ${task.title}`} onPress={() => openTask(topic.id, task.id)}
                  testID={`TamilTask-${task.id}`} trailing={<Icon as={ChevronRight} size={17} color={colors.muted} />}>
                  <View style={styles.row}>
                    <Text style={[styles.badge, { color: tint, backgroundColor: `${tint}23` }]}>{task.difficulty}</Text>
                    <Text numberOfLines={1} style={[styles.rowTitle, { color: colors.foreground }]}>{task.number}. {task.title}</Text>
                  </View>
                  <Text style={{ color: colors.muted, fontSize: 12 }}>{finished ? '✓  Completed' : `${task.exercises.length} exercises`}</Text>
                </DenseActionRow>
                <CourseButton label="Flash cards" testID={`TamilFlashCards-${task.id}`} onPress={() => {
                  Keyboard.dismiss();
                  router.push({ pathname: '/tamil/lessons/[groupId]/[taskId]', params: { groupId: topic.id, taskId: task.id, mode: 'flash-cards' } });
                }} /></View>
              );
            }}
          />
        </>
      )}
      <TamilLibraryDock selected={topicId || sectionId ? undefined : 'All Topics'} />
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
