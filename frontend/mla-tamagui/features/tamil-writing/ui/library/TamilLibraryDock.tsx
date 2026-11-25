import { FloatingBar, FloatingSurface, Icon, useCompactColors } from '@lib/tamagui-design-system';
import { BookOpen, Pencil, Search } from '@lib/tamagui-design-system/icons';
import { router } from 'expo-router';
import { Pressable, Text } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

const tabs = [
  { label: 'Library', icon: BookOpen, path: '/tamil' },
  { label: 'All Topics', icon: Search, path: '/tamil/all-topics' },
  { label: 'Handwriting', icon: Pencil, path: '/tamil/handwriting' },
] as const;

export function TamilLibraryDock({ selected }: { selected?: typeof tabs[number]['label'] }) {
  const colors = useCompactColors();
  const { bottom } = useSafeAreaInsets();
  return <FloatingBar style={{ left: 21, right: 21 }} bottom={Math.max(bottom - 14, 10)}>
    <FloatingSurface tone="overlay" style={{ flex: 1, flexDirection: 'row', borderRadius: 32, padding: 6 }}>
      {tabs.map((tab) => {
        const active = tab.label === selected;
        const color = active ? colors.gold : colors.foreground;
        return <Pressable key={tab.path} accessibilityRole="tab" accessibilityState={{ selected: active }} accessibilityLabel={tab.label}
          testID={tab.label === 'Handwriting' ? 'TamilHandwritingEntry' : `TamilTab-${tab.label}`}
          onPress={() => { if (!active) router.navigate(tab.path); }}
          style={{ flex: 1, minHeight: 48, alignItems: 'center', justifyContent: 'center', borderRadius: 24, backgroundColor: active ? colors.background : 'transparent' }}>
          <Icon as={tab.icon} size={21} color={color} /><Text style={{ color, fontSize: 11, fontWeight: '600' }}>{tab.label}</Text>
        </Pressable>;
      })}
    </FloatingSurface>
  </FloatingBar>;
}
