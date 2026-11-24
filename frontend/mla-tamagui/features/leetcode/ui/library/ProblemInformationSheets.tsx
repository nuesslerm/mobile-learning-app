import { SheetPager, useCompactColors } from '@lib/tamagui-design-system';
import { Sheet } from '@lib/tamagui-design-system/ui/sheet';
import { useState } from 'react';
import { StyleSheet, Text, View } from 'react-native';

export type ProblemInformation = 'topics' | 'companies' | 'hints';

export function ProblemInformationSheet({ kind, topics, companies, hints, onClose }: {
  kind: ProblemInformation;
  topics: readonly string[];
  companies: ReadonlyArray<readonly [string, string, string]>;
  hints: readonly string[];
  onClose: () => void;
}) {
  const colors = useCompactColors();
  const [page, setPage] = useState(0);
  const title = kind === 'topics' ? 'Topics' : kind === 'companies' ? 'Companies' : `Hint ${page + 1}`;
  return (
    <Sheet open onOpenChange={(open) => { if (!open) onClose(); }} snapPoints={kind === 'companies' ? [95] : [53]}
      backdropOpacity={0.52} floating={kind !== 'companies'} floatingHorizontalInset={7} floatingBottomInset={8} cornerRadius={30} bottomCornerRadius={30} accessible={false} handleLabel={`Dismiss ${title}`}
      disableContentPanningGesture={kind === 'hints'} contentStyle={{ backgroundColor: colors.surface }} testID={`ProblemInfo-${kind}`}>
      <Text accessibilityRole="header" testID="ProblemInfoTitle" style={[styles.title, { color: colors.foreground }]}>{title}</Text>
      {kind === 'hints' ? (
        <SheetPager page={page} onPageChange={setPage} pages={hints.map((hint, index) => ({
          id: String(index), label: `Hint ${index + 1}`,
          content: <Text testID={`ProblemHint-${index + 1}`} style={[styles.hint, { color: colors.foreground }]}>{hint}</Text>,
        }))} />
      ) : (
        <Sheet.ScrollView contentContainerStyle={styles.rows}>
          {kind === 'topics' ? topics.map((topic) => (
            <InformationRow key={topic} name={topic} count={topic === 'Array' ? '2,224' : topic === 'Hash Table' ? '844' : undefined}
              glyph={topic === 'Array' ? '▦' : '✥'} tint={topic === 'Array' ? '#808084' : '#60a5fa'} />
          )) : companies.map(([company, count, tint]) => (
            <InformationRow key={company} name={company} count={count} glyph={company[0]} tint={tint} isCompany />
          ))}
        </Sheet.ScrollView>
      )}
    </Sheet>
  );
}

function InformationRow({ name, count, glyph, tint, isCompany }: {
  name: string; count?: string; glyph: string; tint: string; isCompany?: boolean;
}) {
  const colors = useCompactColors();
  return (
    <View style={styles.row}>
      <View style={[styles.tile, { backgroundColor: isCompany ? '#fff' : colors.raised }]}>
        <Text style={{ color: tint, fontSize: 26, fontWeight: '700' }}>{glyph}</Text>
      </View>
      <View>
        <Text style={{ color: colors.foreground, fontSize: 15, fontWeight: '500' }}>{name}</Text>
        {count && <Text style={{ color: colors.muted, fontSize: 12 }}>{count} Questions</Text>}
      </View>
    </View>
  );
}

const styles = StyleSheet.create({
  title: { textAlign: 'center', fontSize: 16, fontWeight: '600', marginTop: 4, marginBottom: 14 },
  rows: { paddingHorizontal: 20, paddingBottom: 36 },
  row: { minHeight: 57, flexDirection: 'row', alignItems: 'center', gap: 13 },
  tile: { width: 41, height: 41, borderRadius: 8, alignItems: 'center', justifyContent: 'center' },
  hint: { fontSize: 15, lineHeight: 19, paddingHorizontal: 10, paddingTop: 7 },
});
