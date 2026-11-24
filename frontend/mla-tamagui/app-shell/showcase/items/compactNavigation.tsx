import {
  CompactHeader, CompactIconButton, CompactTabs, DenseActionRow, FloatingBar, FloatingSurface, SearchPill, SheetPager, useCompactColors,
} from '@lib/tamagui-design-system';
import { MoreHorizontal, Play } from '@lib/tamagui-design-system/icons';
import { Sheet } from '@lib/tamagui-design-system/ui/sheet';
import { Overlay } from '../overlayHost';
import { useState } from 'react';
import { Keyboard, Text, View } from 'react-native';

const tabs = [{ id: 'all', label: 'All' }, { id: 'saved', label: 'Saved' }] as const;
const rows = [{ id: 'one', title: 'Search and sort' }, { id: 'two', title: 'Working with arrays' }];

export function CompactNavigationItem() {
  const colors = useCompactColors();
  const [query, setQuery] = useState('');
  const [tab, setTab] = useState<'all' | 'saved'>('all');
  const [selected, setSelected] = useState(rows[0].title);
  const [open, setOpen] = useState(false);
  const [page, setPage] = useState(0);
  return (
    <View style={{ backgroundColor: colors.background, minHeight: 310, borderRadius: 20, paddingTop: 8 }}>
      <CompactHeader title={selected} onClose={() => { setSelected(rows[0].title); setQuery(''); }} />
      <View style={{ paddingHorizontal: 16 }}>
        <SearchPill value={query} onChangeText={setQuery} onSubmit={Keyboard.dismiss} testID="ShowcaseCompactSearch" />
      </View>
      <CompactTabs items={tabs} value={tab} onChange={setTab} testID="ShowcaseCompactTabs" />
      <View style={{ paddingHorizontal: 16 }}>
        {rows.filter((row) => row.title.toLowerCase().includes(query.toLowerCase()) && (tab === 'all' || row.title === selected)).map((row) => (
          <DenseActionRow key={row.id} label={row.title} onPress={() => setSelected(row.title)}
            trailing={<CompactIconButton label={`Select ${row.title}`} icon={MoreHorizontal} onPress={() => setSelected(row.title)} />}>
            <Text style={{ color: colors.foreground, fontWeight: '600' }}>{row.title}</Text>
            <Text style={{ color: colors.muted }}>Practice collection</Text>
          </DenseActionRow>
        ))}
      </View>
      <FloatingBar bottom={12}>
        <FloatingSurface style={{ paddingHorizontal: 14, paddingVertical: 9 }}>
          <Text style={{ color: colors.foreground }}>{selected}</Text>
        </FloatingSurface>
        <CompactIconButton label="Open page sheet" icon={Play} emphasis onPress={() => { setPage(0); setOpen(true); }} />
      </FloatingBar>
      {open && <Overlay id="showcase-compact-sheet">
        <Sheet open onOpenChange={setOpen} snapPoints={[53]} floating cornerRadius={30} handleLabel="Close page sheet" testID="ShowcaseCompactPageSheet">
          <Text style={{ color: colors.foreground, textAlign: 'center', fontWeight: '600', marginBottom: 16 }}>{rows[page].title}</Text>
          <SheetPager page={page} onPageChange={setPage} pages={rows.map((row) => ({
            id: row.id, label: row.title, content: <Text style={{ color: colors.foreground, paddingHorizontal: 20 }}>{row.title}</Text>,
          }))} />
        </Sheet>
      </Overlay>}
    </View>
  );
}
