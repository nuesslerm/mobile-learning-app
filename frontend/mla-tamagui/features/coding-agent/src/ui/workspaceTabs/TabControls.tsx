import type { ReactNode } from 'react';
import { Pressable, Text, useColorScheme } from 'react-native';

export function useTabColors() {
  return useColorScheme() === 'dark'
    ? {
        bg: '#18181b',
        surface: '#252528',
        ink: '#fafafa',
        muted: '#aaa',
        border: '#47474a',
      }
    : {
        bg: '#fff',
        surface: '#f5f5f5',
        ink: '#171717',
        muted: '#606066',
        border: '#d4d4d4',
      };
}

export function TabControl({
  label,
  children,
  onPress,
  selected,
  disabled,
}: {
  label: string;
  children?: ReactNode;
  onPress: () => void;
  selected?: boolean;
  disabled?: boolean;
}) {
  const c = useTabColors();
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={label}
      accessibilityState={{ disabled, selected }}
      disabled={disabled}
      onPress={onPress}
      hitSlop={2}
      style={({ pressed }) => ({
        minWidth: 30,
        height: 30,
        paddingHorizontal: 7,
        borderRadius: 8,
        alignItems: 'center',
        justifyContent: 'center',
        backgroundColor: selected ? c.ink : c.surface,
        opacity: disabled ? 0.4 : pressed ? 0.65 : 1,
      })}
    >
      {children ?? (
        <Text style={{ fontSize: 13, color: selected ? c.bg : c.ink }}>
          {label}
        </Text>
      )}
    </Pressable>
  );
}
