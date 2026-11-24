import { useCompactColors } from '@lib/tamagui-design-system';
import { Linking, Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';

import type { LibraryLesson } from '../../domain/library/library';

function LessonLink({ label, url }: { label: string; url: string }) {
  const colors = useCompactColors();
  return <Pressable accessibilityRole="link" onPress={() => { void Linking.openURL(url); }} style={{ paddingVertical: 6 }}>
    <Text style={{ color: colors.green, fontSize: 16, fontWeight: '600' }}>{label} ↗</Text>
  </Pressable>;
}

export function NeetCodeLesson({ title, lesson }: { title: string; lesson: LibraryLesson }) {
  const colors = useCompactColors();
  return <ScrollView testID="NeetCodeLesson" contentContainerStyle={styles.content}>
    <Text style={{ color: colors.muted, fontSize: 13 }}>NeetCode 150</Text>
    <Text accessibilityRole="header" style={[styles.title, { color: colors.foreground }]}>{title}</Text>
    {lesson.video && <LessonLink label="Watch video explanation" url={lesson.video} />}
    {lesson.prerequisites.map(({ title: name, url }) => <LessonLink key={url} label={name} url={url} />)}
    {lesson.blocks.map(({ kind, text }, index) => {
      if (kind === 'pre') return <View key={index} style={[styles.code, { backgroundColor: colors.surface }]}>
        <Text style={{ color: colors.muted, fontSize: 12, paddingHorizontal: 12, paddingTop: 8 }}>Python</Text>
        <ScrollView horizontal>
        <Text selectable style={{ color: colors.foreground, padding: 12, fontSize: 13, lineHeight: 19, fontFamily: 'monospace' }}>{text}</Text>
        </ScrollView>
      </View>;
      const heading = kind === 'h2' || kind === 'h3';
      return <Text key={index} accessibilityRole={heading ? 'header' : undefined} selectable
        style={[styles.paragraph, heading && styles.heading, kind === 'h2' && styles.title, { color: colors.foreground }]}>
        {kind === 'li' ? '•  ' : ''}{text}
      </Text>;
    })}
    <View style={[styles.source, { borderColor: colors.line }]}><LessonLink label="Read on NeetCode" url={lesson.url} />
      <LessonLink label="Anki-NeetCode source deck" url="https://github.com/krmanik/Anki-NeetCode" /></View>
  </ScrollView>;
}

const styles = StyleSheet.create({
  content: { padding: 18, paddingBottom: 62, gap: 7 },
  title: { fontSize: 23, lineHeight: 28, fontWeight: '700', marginVertical: 6 },
  heading: { fontSize: 17, fontWeight: '600', marginTop: 12 },
  paragraph: { fontSize: 16, lineHeight: 22 },
  code: { borderRadius: 10, marginVertical: 5 },
  source: { borderTopWidth: StyleSheet.hairlineWidth, marginTop: 12, paddingTop: 8 },
});
