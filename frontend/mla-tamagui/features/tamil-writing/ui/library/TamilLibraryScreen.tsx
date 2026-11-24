import { CompactIconButton, DenseActionRow, Icon, useCompactColors } from '@lib/tamagui-design-system';
import { BookOpen, ChevronRight, Menu, Pencil } from '@lib/tamagui-design-system/icons';
import { router, useNavigation } from 'expo-router';
import { ScrollView, StyleSheet, Text, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { exerciseCount } from '../../domain/lessons/course';
import { useCourse } from '../../state/lessons/useCourse';
import { backTo, CourseLayout, CourseMessage } from './CourseLayout';

export function TamilLibraryScreen() {
  const colors = useCompactColors();
  const navigation = useNavigation();
  const { bottom } = useSafeAreaInsets();
  const { course, error, reload } = useCourse();
  const topics = course?.sections.flatMap((section) => section.topics) ?? [];
  return (
    <CourseLayout title="Tamil" onBack={() => backTo('/')} testID="TamilLibrary"
      leading={<CompactIconButton icon={Menu} label="Open menu" testID="TamilCourseMenuButton"
        onPress={() => navigation.getParent()?.dispatch({ type: 'OPEN_DRAWER' })} />}>
      <ScrollView contentContainerStyle={[styles.content, { paddingBottom: 24 + bottom }]}>
        <Text accessibilityRole="header" style={[styles.heading, { color: colors.foreground }]}>Tamil Library</Text>
        {error && <CourseMessage text="Couldn’t load the Tamil course." retry={() => { void reload(); }} />}
        {!course && !error && <CourseMessage text="Loading lessons…" />}
        {course && (
          <View style={[styles.overview, { backgroundColor: colors.surface }]}>
            <DenseActionRow label="All Topics" testID="TamilAllTopics" onPress={() => router.push('/tamil/all-topics')}
              trailing={<Icon as={ChevronRight} color={colors.muted} size={19} />}>
              <Text style={[styles.sectionTitle, { color: colors.foreground }]}>All Topics</Text>
              <Text style={{ color: colors.muted }}>{topics.length} topics · {topics.reduce((sum, topic) => sum + topic.tasks.length, 0)} tasks · {exerciseCount(topics).toLocaleString('en-US')} exercises</Text>
            </DenseActionRow>
          </View>
        )}
        {course?.sections.map((section) => (
          <View key={section.id} style={styles.section}>
            <Text accessibilityRole="header" style={[styles.sectionTitle, { color: colors.foreground }]}>{section.title}</Text>
            <View style={[styles.group, { backgroundColor: colors.surface }]}>
              {section.topics.map((topic) => (
                <DenseActionRow key={topic.id} label={topic.title} testID={`TamilTopic-${topic.id}`}
                  onPress={() => router.push({ pathname: '/tamil/lessons/[groupId]', params: { groupId: topic.id } })}
                  trailing={<Icon as={ChevronRight} color={colors.muted} size={17} />}>
                  <View style={styles.row}>
                    <Icon as={BookOpen} color={colors.gold} size={19} />
                    <Text style={[styles.label, { color: colors.foreground }]}>{topic.title}</Text>
                    <Text style={{ color: colors.muted }}>{topic.tasks.length} tasks · {exerciseCount([topic])} exercises</Text>
                  </View>
                </DenseActionRow>
              ))}
            </View>
          </View>
        ))}
        <Text style={[styles.credit, { color: colors.muted }]}>Adapted from Learn Tamil Quickly English to Tamil by Charu Mamun.</Text>
      </ScrollView>
      <View style={[styles.dock, { paddingBottom: Math.max(bottom, 12) }]}>
        <View style={[styles.group, { backgroundColor: colors.surface }]}>
          <DenseActionRow label="Handwriting" testID="TamilHandwritingEntry" onPress={() => router.push('/tamil/handwriting')}
            trailing={<Icon as={ChevronRight} color={colors.muted} size={18} />}>
            <View style={styles.row}>
              <Icon as={Pencil} color={colors.gold} size={21} />
              <Text style={[styles.label, { color: colors.foreground }]}>Handwriting</Text>
              <Text style={{ color: colors.muted }}>251 characters</Text>
            </View>
          </DenseActionRow>
        </View>
      </View>
    </CourseLayout>
  );
}

const styles = StyleSheet.create({
  content: { paddingHorizontal: 18, paddingTop: 12, gap: 18 },
  heading: { fontSize: 26, fontWeight: '700' },
  section: { gap: 8 },
  sectionTitle: { fontSize: 18, fontWeight: '700' },
  overview: { borderRadius: 16, paddingHorizontal: 16, paddingVertical: 10 },
  group: { borderRadius: 20, paddingHorizontal: 12 },
  row: { flexDirection: 'row', gap: 12, alignItems: 'center' },
  label: { flex: 1, fontSize: 16, fontWeight: '600' },
  credit: { fontSize: 13, lineHeight: 18 },
  dock: { paddingHorizontal: 18, paddingTop: 8 },
});
