import type { LucideIcon } from 'lucide-react-native';
import type { ComponentRef, ReactNode, Ref } from 'react';
import { Pressable, ScrollView, StyleSheet, Text, TextInput, View, useColorScheme, type StyleProp, type ViewStyle } from 'react-native';

import { ChevronLeft, CircleX, Search, X } from '../icons';
import { usePresetThemeStore } from '../theme/state/presetThemeStore';
import { GlassControlBackground, supportsGlassControl } from '../ui/glass-control-background';
import { Icon } from '../ui/icon';

const darkColors = {
  background: '#000', surface: '#1c1c1e', raised: '#262628', foreground: '#f5f5f7',
  muted: '#929297', line: '#242426', green: '#26d046', gold: '#ffab32', red: '#f24c63',
};
const lightColors: typeof darkColors = {
  background: '#f8f8fa', surface: '#fff', raised: '#ededf0', foreground: '#171719',
  muted: '#66666e', line: '#dddde2', green: '#187c32', gold: '#9a5600', red: '#c62a46',
};

export function useCompactColors() {
  const system = useColorScheme();
  const mode = usePresetThemeStore((state) => state.themeMode);
  return (mode === 'system' ? system : mode) === 'dark' ? darkColors : lightColors;
}

export function FloatingSurface({ children, style, testID, tone = 'regular' }: {
  children: ReactNode;
  style?: StyleProp<ViewStyle>;
  testID?: string;
  tone?: 'regular' | 'subtle';
}) {
  const colors = useCompactColors();
  const glass = supportsGlassControl();
  const fill = tone === 'subtle' && colors === darkColors ? 'rgba(16,16,18,0.82)' : colors.surface;
  return (
    <View testID={testID} style={[styles.surface, { backgroundColor: fill, borderColor: colors.line }, style]}>
      {glass && <GlassControlBackground />}
      {children}
    </View>
  );
}

type CompactIconButtonProps = {
  icon: LucideIcon;
  label: string;
  onPress: () => void;
  testID?: string;
  color?: string;
  filled?: boolean;
  emphasis?: boolean;
  disabled?: boolean;
};

export function CompactIconButton({ icon, label, onPress, testID, color, filled, emphasis, disabled }: CompactIconButtonProps) {
  const colors = useCompactColors();
  const content = <Icon as={icon} size={emphasis ? 21 : 18} color={color ?? (emphasis ? '#fff' : colors.foreground)} filled={filled} />;
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={label}
      accessibilityState={{ disabled }}
      disabled={disabled}
      testID={testID}
      onPress={onPress}
      hitSlop={4}
      style={({ pressed }) => ({ opacity: disabled ? 0.4 : pressed ? 0.65 : 1 })}
    >
      {emphasis ? <View style={[styles.emphasis, { backgroundColor: colors.green }]}>{content}</View> :
        <FloatingSurface style={styles.iconButton}>{content}</FloatingSurface>}
    </Pressable>
  );
}

type SearchPillProps = {
  value: string;
  onChangeText: (text: string) => void;
  onFocus?: () => void;
  onBlur?: () => void;
  onSubmit?: () => void;
  onCancel?: () => void;
  placeholder?: string;
  testID: string;
  inputRef?: Ref<ComponentRef<typeof TextInput>>;
};

export function SearchPill({ value, onChangeText, onFocus, onBlur, onSubmit, onCancel, placeholder = 'Search', testID, inputRef }: SearchPillProps) {
  const colors = useCompactColors();
  return (
    <View style={styles.searchRow}>
      <FloatingSurface style={styles.search} tone="subtle">
        <Icon as={Search} size={15} color={colors.muted} />
        <TextInput
          ref={inputRef}
          accessibilityLabel={placeholder}
          testID={testID}
          value={value}
          placeholder={placeholder}
          placeholderTextColor={colors.muted}
          style={[styles.searchInput, { color: colors.foreground }]}
          autoCorrect={false}
          autoCapitalize="none"
          returnKeyType="search"
          keyboardAppearance={colors === darkColors ? 'dark' : 'light'}
          onFocus={onFocus}
          onBlur={onBlur}
          onChangeText={onChangeText}
          onSubmitEditing={onSubmit}
        />
        {value.length > 0 && <Pressable accessibilityRole="button" accessibilityLabel="Clear search" onPress={() => onChangeText('')} hitSlop={8}>
          <Icon as={CircleX} size={16} color={colors.foreground} />
        </Pressable>}
      </FloatingSurface>
      {onCancel && <CompactIconButton icon={X} label="Cancel search" testID={`${testID}Cancel`} onPress={onCancel} />}
    </View>
  );
}

export function CompactHeader({ title, onClose, trailing, closeIcon = ChevronLeft, closeLabel = 'Back' }: {
  title?: string;
  onClose: () => void;
  trailing?: ReactNode;
  closeIcon?: LucideIcon;
  closeLabel?: string;
}) {
  const colors = useCompactColors();
  return (
    <View style={styles.header}>
      <View style={styles.headerTitle} pointerEvents="none">
        <Text numberOfLines={1} style={{ color: colors.foreground, fontSize: 15, fontWeight: '600' }}>{title}</Text>
      </View>
      <CompactIconButton icon={closeIcon} label={closeLabel} onPress={onClose} />
      <View style={styles.trailing}>{trailing}</View>
    </View>
  );
}

export type CompactTab<T extends string> = { id: T; label: string; accent?: boolean };

export function CompactTabs<T extends string>({ items, value, onChange, testID }: {
  items: ReadonlyArray<CompactTab<T>>;
  value: T;
  onChange: (value: T) => void;
  testID: string;
}) {
  const colors = useCompactColors();
  return (
    <ScrollView horizontal style={styles.tabStrip} showsHorizontalScrollIndicator={false} keyboardShouldPersistTaps="handled" contentContainerStyle={styles.tabs}>
      {items.map(({ id, label, accent }) => (
        <Pressable key={id} testID={`${testID}-${id}`} accessibilityRole="tab" accessibilityState={{ selected: id === value }}
          style={[styles.tab, id === value && { backgroundColor: colors.raised }]} onPress={() => onChange(id)}>
          <Text style={{ color: id === value ? colors.foreground : accent ? colors.gold : colors.muted, fontSize: 13, fontWeight: '600' }}>{label}</Text>
        </Pressable>
      ))}
    </ScrollView>
  );
}

export function DenseActionRow({ children, trailing, onPress, label, testID, density = 'compact' }: {
  children: ReactNode;
  trailing?: ReactNode;
  onPress: () => void;
  label: string;
  testID?: string;
  density?: 'compact' | 'comfortable';
}) {
  const colors = useCompactColors();
  return (
    <View style={[styles.row, { borderColor: colors.line }]}>
      <Pressable testID={testID} accessibilityRole="button" accessibilityLabel={label} onPress={onPress}
        style={({ pressed }) => [styles.rowMain, density === 'comfortable' && styles.comfortableRow, pressed && { backgroundColor: colors.surface }]}>
        {children}
      </Pressable>
      {trailing && <View style={styles.rowTrailing}>{trailing}</View>}
    </View>
  );
}

export function FloatingBar({ children, testID, bottom = 0, style }: {
  children: ReactNode;
  testID?: string;
  bottom?: number;
  style?: StyleProp<ViewStyle>;
}) {
  return <View testID={testID} pointerEvents="box-none" style={[styles.bar, { bottom }, style]}>{children}</View>;
}

const styles = StyleSheet.create({
  surface: { borderRadius: 22, borderWidth: StyleSheet.hairlineWidth, overflow: 'hidden' },
  iconButton: { height: 42, width: 42, borderRadius: 21, alignItems: 'center', justifyContent: 'center' },
  emphasis: { height: 56, width: 56, borderRadius: 28, alignItems: 'center', justifyContent: 'center' },
  searchRow: { flexDirection: 'row', alignItems: 'center', gap: 8 },
  search: { flex: 1, minHeight: 39, paddingHorizontal: 12, flexDirection: 'row', alignItems: 'center', gap: 6 },
  searchInput: { flex: 1, minHeight: 39, paddingVertical: 4, fontSize: 15 },
  header: { paddingHorizontal: 18, minHeight: 48, flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between' },
  headerTitle: { position: 'absolute', left: 100, right: 100, alignItems: 'center' },
  trailing: { flexDirection: 'row', alignItems: 'center', gap: 6 },
  tabStrip: { flexGrow: 0, flexShrink: 0 },
  tabs: { paddingHorizontal: 18, gap: 3, alignItems: 'center', paddingVertical: 6 },
  tab: { borderRadius: 17, minHeight: 32, paddingHorizontal: 11, alignItems: 'center', justifyContent: 'center' },
  row: { flexDirection: 'row', alignItems: 'stretch', borderBottomWidth: StyleSheet.hairlineWidth },
  rowMain: { flex: 1, paddingVertical: 10, minHeight: 68, justifyContent: 'center', gap: 6 },
  comfortableRow: { minHeight: 81 },
  rowTrailing: { justifyContent: 'center', alignItems: 'center' },
  bar: { position: 'absolute', left: 18, right: 18, flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between' },
});
