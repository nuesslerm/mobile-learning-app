import { useAgentTheme, type WorkspaceTabControlProps } from '@lib/agent-ui-contracts';
import { Pressable, Text } from 'react-native';

export function WorkspaceTabControl({
  label,
  children,
  terminal,
  browser,
  display,
  onPress,
  selected,
  disabled,
}: WorkspaceTabControlProps) {
  const c = useAgentTheme().tabs;
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
