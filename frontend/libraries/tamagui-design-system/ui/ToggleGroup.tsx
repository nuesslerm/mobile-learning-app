import * as React from 'react';

import {
  ToggleGroup as TamaguiToggleGroup,
  type ToggleGroupItemProps as TamaguiToggleGroupItemProps,
  type ToggleGroupProps as TamaguiToggleGroupProps,
} from '../primitives';

export type ToggleGroupProps = TamaguiToggleGroupProps & {
  /** Enables the pill-track layout app #1 calls `segmented`. */
  segmented?: boolean;
  /**
   * Kept for call-site parity with app #1's toggleVariants. The only visual
   * difference there is a drop shadow, and shadows are the grammar this
   * design system drops -- so both values render identically.
   */
  variant?: 'default' | 'outline';
  size?: 'default' | 'sm' | 'lg';
};

export type ToggleGroupItemProps = TamaguiToggleGroupItemProps & {
  /** Accepted for call-site parity; absorbed by the root's rounding. */
  isFirst?: boolean;
  isLast?: boolean;
};

// app #1's toggleVariants heights (h-9 / h-8 / h-10) on the kept v4 size
// scale: $3 is 36, $2.5 is 32, $3.5 is 40.
const SIZE_HEIGHT = {
  default: '$3',
  sm: '$2.5',
  lg: '$3.5',
} as const;

type ToggleGroupContextValue = {
  value: string | string[] | undefined;
  segmented: boolean;
  size: NonNullable<ToggleGroupProps['size']>;
};

const ToggleGroupContext = React.createContext<ToggleGroupContextValue | null>(
  null
);

function useToggleGroupContext() {
  const context = React.useContext(ToggleGroupContext);
  if (context === null) {
    throw new Error('ToggleGroupItem cannot be rendered outside a ToggleGroup');
  }
  return context;
}

/**
 * ToggleGroup on Tamagui's own ToggleGroup primitive (`type`, `value`, `onValueChange`, `segmented`,
 * compound `ToggleGroupItem`).
 *
 * Selection state is derived from the root's value through the context
 * below -- the same architecture app #1 uses against its primitive. The
 * shadcn group look (transparent rail, hairline border, rounded shell)
 * comes from the root; the active item's background is the only stateful
 * colour. The segmented pill track follows this design system's
 * SegmentedControl grammar (muted rail, accent-filled active pill); the
 * Duolingo 3D underlay app #1 renders there stays behind.
 */
export function ToggleGroup({
  segmented = false,
  size = 'default',
  children,
  ...props
}: ToggleGroupProps) {
  const { value } = props;

  return (
    <ToggleGroupContext.Provider value={{ value, segmented, size }}>
      <TamaguiToggleGroup
        flexDirection="row"
        items="center"
        rounded={segmented ? '$4' : '$md'}
        overflow="hidden"
        flex={segmented ? 1 : undefined}
        bg={segmented ? '$color3' : 'transparent'}
        p={segmented ? '$1' : undefined}
        gap={segmented ? '$1' : undefined}
        borderWidth={1}
        borderColor="$borderColor"
        {...props}
      >
        {children}
      </TamaguiToggleGroup>
    </ToggleGroupContext.Provider>
  );
}

export function ToggleGroupItem({
  isFirst: _isFirst,
  isLast: _isLast,
  value,
  children,
  ...props
}: ToggleGroupItemProps) {
  const context = useToggleGroupContext();
  const isSelected = Array.isArray(context.value)
    ? context.value.includes(value)
    : context.value === value;

  if (context.segmented) {
    return (
      <TamaguiToggleGroup.Item
        unstyled
        flex={1}
        flexDirection="row"
        items="center"
        justify="center"
        gap="$1"
        py="$2"
        rounded="$3"
        bg={isSelected ? '$color9' : 'transparent'}
        pressStyle={{ opacity: 0.7 }}
        disabledStyle={{ opacity: 0.5 }}
        value={value}
        {...props}
      >
        {children}
      </TamaguiToggleGroup.Item>
    );
  }

  return (
    <TamaguiToggleGroup.Item
      unstyled
      flex={1}
      flexDirection="row"
      items="center"
      justify="center"
      gap="$1"
      height={SIZE_HEIGHT[context.size]}
      px="$4"
      bg={isSelected ? '$accent' : 'transparent'}
      pressStyle={{ opacity: 0.7 }}
      disabledStyle={{ opacity: 0.5 }}
      value={value}
      {...props}
    >
      {children}
    </TamaguiToggleGroup.Item>
  );
}
