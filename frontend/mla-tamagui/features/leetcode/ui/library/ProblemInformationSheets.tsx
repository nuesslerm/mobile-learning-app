import { SheetPager, useCompactColors } from '@lib/tamagui-design-system';
import { Sheet } from '@lib/tamagui-design-system/ui/sheet';
import { useState } from 'react';
import { StyleSheet, Text } from 'react-native';

export type ProblemInformation = 'topics' | 'hints';

export function ProblemInformationSheet({ kind, topics, hints, onClose }: {
  kind: ProblemInformation;
  topics: readonly string[];
  hints: readonly string[];
  onClose: () => void;
}) {
  const colors = useCompactColors();
  const [page, setPage] = useState(0);
  const title = kind === 'topics' ? 'Topics' : `Hint ${page + 1}`;
  return (
    <Sheet open onOpenChange={(open) => { if (!open) onClose(); }} snapPoints={[53]}
      backdropOpacity={0.52} floating floatingHorizontalInset={7} floatingBottomInset={8} cornerRadius={30} bottomCornerRadius={30} accessible={false} handleLabel={`Dismiss ${title}`}
      disableContentPanningGesture={kind === 'hints'} contentStyle={{ backgroundColor: colors.surface }} testID={`ProblemInfo-${kind}`}>
      <Text accessibilityRole="header" testID="ProblemInfoTitle" style={[styles.title, { color: colors.foreground }]}>{title}</Text>
      {kind === 'hints' ? (
        <SheetPager page={page} onPageChange={setPage} pages={hints.map((hint, index) => ({
          id: String(index), label: `Hint ${index + 1}`,
          content: <Text testID={`ProblemHint-${index + 1}`} style={[styles.hint, { color: colors.foreground }]}>{hint}</Text>,
        }))} />
      ) : (
        <Sheet.ScrollView contentContainerStyle={styles.rows}>
          {topics.map((topic) => <Text key={topic} style={[styles.topic, { color: colors.foreground }]}>{topic}</Text>)}
        </Sheet.ScrollView>
      )}
    </Sheet>
  );
}

const styles = StyleSheet.create({
  title: { textAlign: 'center', fontSize: 16, fontWeight: '600', marginTop: 4, marginBottom: 14 },
  rows: { paddingHorizontal: 20, paddingBottom: 36 },
  topic: { fontSize: 15, paddingVertical: 12 },
  hint: { fontSize: 15, lineHeight: 19, paddingHorizontal: 10, paddingTop: 7 },
});
