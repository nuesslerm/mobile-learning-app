import type { WorkspaceTabControlProps } from '@lib/skin';
import { Text } from '@lib/tamagui-design-system';
import { XStack } from '@lib/tamagui-design-system/primitives';

/** One key or tool of a workspace tab's control rows: a small pill, filled when it is the one in effect. */
export function WorkspaceTabControl({ label, children, terminal, browser, display, onPress, selected, disabled }: WorkspaceTabControlProps) {
  const tool = browser === 'tool';
  const mono = terminal || browser === 'key';
  return (
    <XStack
      role="button"
      accessible
      aria-label={display ?? label}
      aria-selected={selected}
      aria-disabled={disabled}
      disabled={disabled}
      hitSlop={2}
      minW={tool ? 26 : browser === 'key' ? 42 : terminal ? 34 : 30}
      height={tool ? 26 : terminal ? 32 : 30}
      px={tool ? 3 : terminal ? 9 : 8}
      rounded={12}
      items="center"
      justify="center"
      bg={selected ? '$color' : tool ? 'transparent' : '$color4'}
      opacity={disabled ? 0.4 : 1}
      pressStyle={{ opacity: 0.65, ...(tool && !selected ? { bg: '$color4' } : {}) }}
      cursor="pointer"
      onPress={onPress}
    >
      {children ?? (
        <Text variant={mono ? 'code' : 'default'} fontSize={browser === 'key' ? 12 : 13} fontWeight={terminal ? '500' : undefined} color={selected ? '$background' : '$color'}>
          {display ?? label}
        </Text>
      )}
    </XStack>
  );
}
