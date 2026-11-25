import { Logger } from '@lib/logging';
import { CompactHeader, CompactIconButton, DenseActionRow, SearchPill, useCompactColors } from '@lib/tamagui-design-system';
import { Camera, Check, X } from '@lib/tamagui-design-system/icons';
import { Sheet } from '@lib/tamagui-design-system/ui/sheet';
import { useState } from 'react';
import { Image, Keyboard, StyleSheet, Switch, Text, TextInput, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import type { SavedProblemList } from '../../domain/library/library';
import { useLibrary } from '../../state/library/useLibrary';
import { useLeetCodePorts } from '../../state/ports';

export function SavedListSheets({ problemId, onClose }: { problemId: string; onClose: () => void }) {
  const { lists, actions } = useLibrary();
  const [query, setQuery] = useState('');
  const [creating, setCreating] = useState(false);
  const [saveFailed, setSaveFailed] = useState(false);
  const colors = useCompactColors();
  const { bottom } = useSafeAreaInsets();
  return (
    <>
      <Sheet open onOpenChange={(open) => { if (!open && !creating) onClose(); }} snapPoints={[95, 47]} cornerRadius={30}
        backdropOpacity={0.42} accessible={false} contentStyle={{ backgroundColor: colors.surface }} testID="AlgorithmsAddToList">
        <CompactHeader title="Add to List" closeIcon={X} closeLabel="Close Add to List" onClose={onClose} />
        <Sheet.ScrollView keyboardShouldPersistTaps="handled" contentContainerStyle={styles.listContent}>
          <DenseActionRow label="New List" testID="AlgorithmsNewList" onPress={() => setCreating(true)}>
            <View style={styles.listLabel}>
              <View style={[styles.cover, { backgroundColor: colors.raised }]}><Text style={{ color: colors.foreground, fontSize: 28 }}>+</Text></View>
              <Text style={{ color: colors.foreground, fontSize: 16 }}>New List</Text>
            </View>
          </DenseActionRow>
          <Text style={[styles.section, { color: colors.foreground }]}>My Lists</Text>
          {lists.filter((list) => list.name.toLowerCase().includes(query.trim().toLowerCase())).map((list) => {
            const member = list.problemIds.includes(problemId);
            return (
              <DenseActionRow key={list.id} label={member ? `${list.name}, already in the list` : list.name} testID={`AlgorithmsSavedList-${list.id}`}
                onPress={() => { if (!member) void actions.save(list.id, problemId).then((saved) => { if (saved) onClose(); else setSaveFailed(true); }); }}>
                <View style={styles.listLabel}>
                  {list.coverUri ? <Image source={{ uri: list.coverUri }} style={styles.cover} /> :
                    <View style={[styles.cover, { backgroundColor: '#f2f2f7' }]}><Text style={{ fontSize: 23 }}>{list.id === 'favorite' ? '⭐' : '🗒️'}</Text></View>}
                  <View style={{ flex: 1, gap: 3 }}>
                    <Text numberOfLines={1} style={{ color: colors.foreground, fontSize: 16 }}>{list.name}</Text>
                    {member && <Text style={{ color: colors.muted, fontSize: 13 }}>Already in the list</Text>}
                  </View>
                </View>
              </DenseActionRow>
            );
          })}
        </Sheet.ScrollView>
        <View style={{ paddingHorizontal: 16, paddingBottom: Math.max(bottom, 12) }}>
          {saveFailed && <Text style={{ color: colors.red }}>Couldn’t add the problem. Try again.</Text>}
          <SearchPill value={query} onChangeText={setQuery} onSubmit={Keyboard.dismiss} testID="AlgorithmsSavedListSearch" />
        </View>
      </Sheet>
      {creating && <NewListSheet onClose={() => setCreating(false)} create={(values) => actions.create(values)} />}
    </>
  );
}

function NewListSheet({ onClose, create }: { onClose: () => void; create: (values: Omit<SavedProblemList, 'id' | 'problemIds'>) => Promise<boolean> }) {
  const { libraryCover } = useLeetCodePorts();
  const colors = useCompactColors();
  const formSurface = colors.background === '#000' ? '#111113' : colors.surface;
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [isPrivate, setPrivate] = useState(false);
  const [coverUri, setCover] = useState<string>();
  const [saving, setSaving] = useState(false);
  const [coverError, setCoverError] = useState(false);
  const [saveError, setSaveError] = useState(false);
  const chooseCover = async () => {
    try {
      setCoverError(false);
      const uri = await libraryCover.pick();
      if (uri) setCover(uri);
    } catch (error) {
      Logger.withError(error).error('leetcode.library.cover_failed');
      setCoverError(true);
    }
  };
  const submit = () => {
    if (!name.trim() || saving) return;
    setSaving(true);
    setSaveError(false);
    void create({ name: name.trim(), description: description.trim(), private: isPrivate, coverUri }).then((saved) => {
      if (saved) onClose();
      else { setSaving(false); setSaveError(true); }
    });
  };
  return (
    <Sheet open onOpenChange={(open) => { if (!open) onClose(); }} snapPoints={[95]} cornerRadius={30} handleIndicatorStyle={{ opacity: 0 }}
      backdropOpacity={0.5} accessible={false} glassOverlayColor={formSurface} contentStyle={{ backgroundColor: formSurface }} testID="AlgorithmsCreateList">
      <CompactHeader title="New List" closeIcon={X} closeLabel="Close New List" onClose={onClose}
        trailing={<CompactIconButton icon={Check} label="Create list" testID="AlgorithmsConfirmNewList" onPress={submit} disabled={!name.trim() || saving} />} />
      <Sheet.ScrollView contentContainerStyle={styles.form} keyboardShouldPersistTaps="handled">
        <View style={[styles.largeCover, { backgroundColor: colors.background === '#000' ? '#303034' : colors.raised }]}>
          {coverUri && <Image source={{ uri: coverUri }} style={StyleSheet.absoluteFill} />}
          <CompactIconButton icon={Camera} label="Choose list cover" emphasis onPress={() => { void chooseCover(); }} />
        </View>
        {saveError && <Text style={{ color: colors.red }}>Couldn’t create the list. Try again.</Text>}
        {coverError && <Text style={{ color: colors.red }}>Couldn’t open the image picker.</Text>}
        <TextInput accessibilityLabel="List name" placeholder="Enter a list name" placeholderTextColor={colors.muted}
          testID="AlgorithmsListName" value={name} onChangeText={setName} style={[styles.input, { textAlign: 'center', color: colors.foreground, borderColor: colors.line }]} />
        <TextInput accessibilityLabel="List description" placeholder="Describe your list" placeholderTextColor={colors.muted}
          value={description} onChangeText={setDescription} style={[styles.input, { color: colors.foreground, borderColor: colors.line }]} />
        <View style={styles.privacy}>
          <Text style={{ color: colors.foreground, fontSize: 15 }}>Set as Private</Text>
          <Switch accessibilityLabel="Set as Private" value={isPrivate} onValueChange={setPrivate} />
        </View>
      </Sheet.ScrollView>
    </Sheet>
  );
}

const styles = StyleSheet.create({
  listContent: { paddingHorizontal: 16, paddingBottom: 16 },
  listLabel: { flexDirection: 'row', alignItems: 'center', gap: 12 },
  cover: { width: 46, height: 46, borderRadius: 7, justifyContent: 'center', alignItems: 'center' },
  section: { fontSize: 16, fontWeight: '600', marginTop: 14, marginBottom: 4 },
  form: { paddingHorizontal: 16, gap: 10 },
  largeCover: { width: 164, height: 164, alignSelf: 'center', alignItems: 'center', justifyContent: 'center', borderRadius: 4, marginBottom: 28, overflow: 'hidden' },
  input: { borderBottomWidth: StyleSheet.hairlineWidth, minHeight: 42, fontSize: 15 },
  privacy: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between' },
});
