import { useCompactColors } from '@lib/tamagui-design-system';
import Constants from 'expo-constants';
import { useState } from 'react';
import { Linking, Platform, Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';
import { WebView } from 'react-native-webview';

import type { LibraryLesson } from '../../domain/library/library';

function LessonLink({ label, url }: { label: string; url: string }) {
  const colors = useCompactColors();
  return <Pressable accessibilityRole="link" onPress={() => { void Linking.openURL(url); }} style={{ paddingVertical: 6 }}>
    <Text style={{ color: colors.green, fontSize: 16, fontWeight: '600' }}>{label} ↗</Text>
  </Pressable>;
}

function LessonVideo({ url, title }: { url: string; title: string }) {
  const colors = useCompactColors();
  const [opened, setOpened] = useState(false);
  const videoId = /^https:\/\/www\.youtube\.com\/watch\?v=([A-Za-z0-9_-]{11})$/.exec(url)?.[1];
  if (!videoId) return <LessonLink label="Watch on YouTube" url={url} />;
  if (!opened) return <Pressable accessibilityRole="button" testID="LessonOpenVideo" onPress={() => setOpened(true)}
    style={{ paddingVertical: 10 }}><Text style={{ color: colors.green, fontSize: 16, fontWeight: '600' }}>Watch video explanation</Text></Pressable>;

  const embed = `https://www.youtube.com/embed/${videoId}?playsinline=1`;
  const appId = Platform.OS === 'ios' ? Constants.expoConfig?.ios?.bundleIdentifier : Constants.expoConfig?.android?.package;
  return <>
    <View style={{ width: '100%', aspectRatio: 16 / 9, minHeight: 200, backgroundColor: 'black' }}>
      {Platform.OS === 'web'
        ? <iframe title={`${title} video explanation`} src={embed} allowFullScreen referrerPolicy="strict-origin-when-cross-origin"
            style={{ border: 0, width: '100%', height: '100%' }} />
        : <WebView testID="LessonVideoPlayer" accessibilityLabel={`${title} video explanation`} source={{
            uri: embed, headers: appId ? { Referer: `https://${appId.toLowerCase()}/` } : undefined,
          }} allowsInlineMediaPlayback allowsFullscreenVideo style={{ flex: 1 }} />}
    </View>
    <LessonLink label="Watch on YouTube" url={url} />
  </>;
}

export function NeetCodeLesson({ title, lesson }: { title: string; lesson: LibraryLesson }) {
  const colors = useCompactColors();
  return <ScrollView testID="NeetCodeLesson" contentContainerStyle={styles.content}>
    <Text style={{ color: colors.muted, fontSize: 13 }}>NeetCode 150</Text>
    <Text accessibilityRole="header" style={[styles.title, { color: colors.foreground }]}>{title}</Text>
    {lesson.video && <LessonVideo key={lesson.video} title={title} url={lesson.video} />}
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
