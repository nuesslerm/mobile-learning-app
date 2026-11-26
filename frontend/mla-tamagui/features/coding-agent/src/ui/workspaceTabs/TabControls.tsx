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
  terminal,
  browser,
  display,
  onPress,
  selected,
  disabled,
}: {
  label: string;
  children?: ReactNode;
  terminal?: boolean;
  browser?: 'tool' | 'key';
  display?: string;
  onPress: () => void;
  selected?: boolean;
  disabled?: boolean;
}) {
  const c = useTabColors();
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={display ?? label}
      accessibilityState={{ disabled, selected }}
      disabled={disabled}
      onPress={onPress}
      hitSlop={2}
      style={({ pressed }) => ({
        minWidth:
          browser === 'tool' ? 26 : browser === 'key' ? 42 : terminal ? 34 : 30,
        height: browser === 'tool' ? 26 : terminal ? 32 : 30,
        paddingHorizontal: browser === 'tool' ? 3 : terminal ? 9 : 8,
        borderRadius: 12,
        alignItems: 'center',
        justifyContent: 'center',
        backgroundColor: selected
          ? c.ink
          : browser === 'tool' && !pressed
            ? 'transparent'
            : c.surface,
        opacity: disabled ? 0.4 : pressed ? 0.65 : 1,
      })}
    >
      {children ?? (
        <Text
          style={{
            fontSize: browser === 'key' ? 12 : 13,
            fontFamily: terminal || browser === 'key' ? 'Menlo' : undefined,
            fontWeight: terminal ? '500' : undefined,
            color: selected ? c.bg : c.ink,
          }}
        >
          {display ?? label}
        </Text>
      )}
    </Pressable>
  );
}
