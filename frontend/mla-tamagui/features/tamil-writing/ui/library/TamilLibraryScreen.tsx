import { CompactIconButton, Icon, useCompactColors } from '@lib/tamagui-design-system';
import { BookOpen, ChevronRight, Menu } from '@lib/tamagui-design-system/icons';
import { router, useNavigation } from 'expo-router';
import { Pressable, ScrollView, StyleSheet, Text, View, useWindowDimensions } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { exerciseCount } from '../../domain/lessons/course';
import { useCourse } from '../../state/lessons/useCourse';
import { CourseMessage } from './CourseLayout';
import { TamilLibraryDock } from './TamilLibraryDock';

const covers = ['#493984', '#1b655a', '#8a4148', '#24567e', '#6e4830'];
const accents = ['#bda4ff', '#78e4cd', '#ffabac', '#8ed1ff', '#fac98c'];

export function TamilLibraryScreen() {
  const colors = useCompactColors();
  const navigation = useNavigation();
  const { top, bottom } = useSafeAreaInsets();
  const { width } = useWindowDimensions();
  const { course, error, reload } = useCourse();
  const topics = course?.sections.flatMap((section) => section.topics) ?? [];
  const bannerWidth = Math.min(440, width * 0.83);
  const openSection = (section: string) => router.push({ pathname: '/tamil/all-topics', params: { section } });

  return <View testID="TamilLibrary" style={{ flex: 1, backgroundColor: colors.background, paddingTop: top }}>
    <View style={styles.header}>
      <CompactIconButton icon={Menu} label="Open menu" testID="TamilCourseMenuButton"
        onPress={() => navigation.dispatch({ type: 'OPEN_DRAWER' })} />
      <Text accessibilityRole="header" style={[styles.title, { color: colors.foreground }]}>Tamil Library</Text>
    </View>
    <ScrollView showsVerticalScrollIndicator={false} contentContainerStyle={{ gap: 20, paddingBottom: bottom + 100 }}>
      {error && <CourseMessage text="Couldn’t load the Tamil course." retry={() => { void reload(); }} />}
      {!course && !error && <CourseMessage text="Loading lessons…" />}
      {course && <>
        <ScrollView horizontal showsHorizontalScrollIndicator={false} snapToInterval={bannerWidth + 10} decelerationRate="fast" contentContainerStyle={styles.rail}>
          {course.sections.map((section, index) => {
            const sample = section.topics[0]?.tasks[0]?.exercises[0];
            return <Pressable key={section.id} accessibilityRole="button" accessibilityLabel={section.title}
              testID={`TamilCollection-${section.id}`} onPress={() => openSection(section.id)}
              style={[styles.cover, { width: bannerWidth, backgroundColor: covers[index % covers.length] }]}>
              {sample && <Text numberOfLines={1} accessibilityLanguage="ta-IN" style={[styles.sample, { color: accents[index % accents.length] }]}>{sample.tamil}</Text>}
              <Text style={styles.coverTitle}>{section.title}</Text>
              <Text style={styles.coverCount}>{section.topics.length} topics · {exerciseCount(section.topics)} exercises</Text>
            </Pressable>;
          })}
        </ScrollView>
        <View style={styles.section}>
          <Text accessibilityRole="header" style={[styles.label, { color: colors.foreground }]}>Categories</Text>
          <ScrollView horizontal showsHorizontalScrollIndicator={false} contentContainerStyle={styles.rail}>
            <Pressable accessibilityRole="button" testID="TamilAllTopics" onPress={() => router.push('/tamil/all-topics')}
              style={[styles.tile, { backgroundColor: colors.surface }]}>
              <Icon as={BookOpen} size={22} color={colors.gold} /><Text style={[styles.tileName, { color: colors.foreground }]}>All Topics</Text>
            </Pressable>
            {course.sections.map((section, index) => <Pressable key={section.id} accessibilityRole="button"
              onPress={() => openSection(section.id)} style={[styles.tile, { backgroundColor: colors.surface }]}>
              <Icon as={BookOpen} size={22} color={accents[index % accents.length]} />
              <Text style={[styles.tileName, { color: colors.foreground }]}>{section.title}</Text>
            </Pressable>)}
          </ScrollView>
        </View>
        <View style={styles.section}>
          <Pressable accessibilityRole="button" onPress={() => router.push('/tamil/all-topics')} style={styles.link}>
            <Text accessibilityRole="header" style={[styles.label, { color: colors.foreground }]}>Topics</Text>
            <Icon as={ChevronRight} size={16} color={colors.muted} />
          </Pressable>
          <View style={styles.chips}>{topics.map((topic) => <Pressable key={topic.id} accessibilityRole="button"
            testID={`TamilTopic-${topic.id}`} style={[styles.chip, { backgroundColor: colors.surface }]}
            onPress={() => router.push({ pathname: '/tamil/lessons/[groupId]', params: { groupId: topic.id } })}>
            <Text style={{ color: colors.foreground, fontWeight: '500' }}>{topic.title}</Text>
          </Pressable>)}</View>
        </View>
        <Text style={[styles.note, { color: colors.muted }]}>{topics.length} topics · {topics.reduce((sum, topic) => sum + topic.tasks.length, 0)} tasks · {exerciseCount(topics).toLocaleString('en-US')} exercises</Text>
        <Text style={[styles.note, { color: colors.muted }]}>Adapted from Learn Tamil Quickly English to Tamil by Charu Mamun.</Text>
      </>}
    </ScrollView>
    <TamilLibraryDock selected="Library" />
  </View>;
}

const styles = StyleSheet.create({
  header: { flexDirection: 'row', alignItems: 'center', gap: 12, paddingHorizontal: 16, paddingVertical: 8 },
  title: { fontSize: 30, fontWeight: '700' },
  rail: { gap: 10, paddingHorizontal: 16 },
  cover: { minHeight: 155, borderRadius: 14, padding: 18, justifyContent: 'flex-end', overflow: 'hidden' },
  sample: { fontWeight: '700', fontSize: 29, marginBottom: 10 },
  coverTitle: { color: '#fff', fontSize: 24, fontWeight: '700' },
  coverCount: { color: '#fff', marginTop: 3 },
  section: { gap: 10 },
  label: { marginLeft: 16, fontSize: 20, fontWeight: '700' },
  tile: { borderRadius: 11, minWidth: 105, paddingHorizontal: 13, paddingVertical: 11, gap: 7 },
  tileName: { fontWeight: '600' },
  chips: { flexDirection: 'row', flexWrap: 'wrap', gap: 8, marginHorizontal: 16 },
  chip: { borderRadius: 18, paddingHorizontal: 13, paddingVertical: 7 },
  link: { flexDirection: 'row', alignItems: 'center', gap: 4 },
  note: { fontSize: 13, marginHorizontal: 16 },
});
