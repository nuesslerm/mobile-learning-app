import { DenseActionRow, Icon, SearchPill, useCompactColors } from '@lib/tamagui-design-system';
import { ChevronDown, ChevronRight } from '@lib/tamagui-design-system/icons';
import { router } from 'expo-router';
import { useState } from 'react';
import { Keyboard, ScrollView, StyleSheet, Text, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { characterGroups, matchesCharacter } from '../../domain/handwriting/catalog';
import { backTo, CourseLayout, CourseMessage } from '../library/CourseLayout';

export function HandwritingLibrary() {
  const colors = useCompactColors();
  const { bottom } = useSafeAreaInsets();
  const [query, setQuery] = useState('');
  const [expanded, setExpanded] = useState<string | null>('vowels');
  const searching = Boolean(query.trim());
  const groups = characterGroups.map((group) => ({
    ...group, characters: group.characters.filter((item) => matchesCharacter(item, query)),
  })).filter((group) => group.characters.length);

  return (
    <CourseLayout title="Handwriting" testID="TamilHandwritingCatalog" onBack={() => backTo('/tamil')}>
      <View style={styles.search}>
        <SearchPill value={query} onChangeText={setQuery} onSubmit={Keyboard.dismiss}
          onCancel={searching ? () => { setQuery(''); Keyboard.dismiss(); } : undefined}
          testID="TamilHandwritingSearch" />
      </View>
      <ScrollView keyboardShouldPersistTaps="handled" contentContainerStyle={[styles.content, { paddingBottom: bottom + 20 }]}>
        {groups.map((group) => {
          const open = searching || expanded === group.id;
          return (
            <View key={group.id}>
              <DenseActionRow label={group.title} onPress={() => setExpanded(open ? null : group.id)}
                trailing={<Icon as={open ? ChevronDown : ChevronRight} size={18} color={colors.muted} />}>
                <View style={styles.row}>
                  <Text style={[styles.title, { color: colors.foreground }]}>{group.title}</Text>
                  <Text style={{ color: colors.muted }}>{group.characters.length}</Text>
                </View>
              </DenseActionRow>
              {open && group.characters.map((item) => (
                <DenseActionRow key={item.id} label={`${item.glyph} ${item.transliteration}`} density="comfortable"
                  testID={`TamilCharacter-${item.id}`}
                  onPress={() => {
                    Keyboard.dismiss();
                    router.push({ pathname: '/tamil/write/[subjectId]', params: { subjectId: item.id } });
                  }}>
                  <View style={styles.row}>
                    <Text style={[styles.glyph, { color: colors.foreground }]} accessibilityLanguage="ta-IN">{item.glyph}</Text>
                    <Text style={[styles.title, { color: colors.muted }]}>{item.transliteration}</Text>
                  </View>
                </DenseActionRow>
              ))}
            </View>
          );
        })}
        {!groups.length && <CourseMessage text="No characters match your search." />}
        <DenseActionRow label="Alphabet lessons" onPress={() => router.push('/tamil/alphabet')}>
          <Text style={[styles.title, { color: colors.foreground }]}>Alphabet lessons</Text>
        </DenseActionRow>
      </ScrollView>
    </CourseLayout>
  );
}

const styles = StyleSheet.create({
  search: { paddingHorizontal: 18, paddingVertical: 5 },
  content: { paddingHorizontal: 18 },
  row: { flexDirection: 'row', alignItems: 'center', gap: 12 },
  title: { flex: 1, fontSize: 16, fontWeight: '600' },
  glyph: { minWidth: 48, textAlign: 'center', fontSize: 22 },
});
