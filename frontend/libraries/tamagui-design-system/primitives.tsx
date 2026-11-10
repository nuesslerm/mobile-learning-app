// The Tamagui pieces this app uses, imported from their own packages. The `tamagui` barrel is `export *` over about
// fifty component packages, which Metro cannot drop: all of them were bundled and their top level ran at start-up.
import './primitives-setup';

import { TamaguiProvider as CoreTamaguiProvider, Text as CoreText, styled } from '@tamagui/core';
import { PortalProvider } from '@tamagui/portal';
import { ZIndexStackContext } from '@tamagui/z-index-stack';
import type { ComponentProps } from 'react';

export {
  createFont,
  createTamagui,
  Theme,
  useTheme,
  View,
  type ColorTokens,
} from '@tamagui/core';
export { AlertDialog } from '@tamagui/alert-dialog';
export { Button, type ButtonProps } from '@tamagui/button';
export { Input, TextArea, type InputProps, type TextAreaProps } from '@tamagui/input';
export { Label } from '@tamagui/label';
export { PortalProvider } from '@tamagui/portal';
export { Popover } from '@tamagui/popover';
export { ScrollView } from '@tamagui/scroll-view';
export { Separator } from '@tamagui/separator';
export { Circle } from '@tamagui/shapes';
export { XStack, YStack, type XStackProps, type YStackProps } from '@tamagui/stacks';
export { Tabs, useTabsContext, type TabsContentProps, type TabsListProps, type TabsProps, type TabsTabProps } from '@tamagui/tabs';
export { SizableText, type SizableTextProps } from '@tamagui/text';
export { ToggleGroup, type ToggleGroupItemProps, type ToggleGroupProps } from '@tamagui/toggle-group';
export { styled };

// What `tamagui` adds to core's Text and Provider, copied so behaviour does not change.
export const Text = styled(CoreText, {
  variants: { unstyled: { false: { color: '$color' } } },
  // styled() infers no values for a variant declared next to `unstyled`, so the literal needs the cast.
  defaultVariants: { unstyled: false } as never,
});

export function TamaguiProvider({ children, ...props }: ComponentProps<typeof CoreTamaguiProvider>) {
  return (
    <CoreTamaguiProvider {...props}>
      <ZIndexStackContext.Provider value={1}>
        <PortalProvider shouldAddRootHost>{children}</PortalProvider>
      </ZIndexStackContext.Provider>
    </CoreTamaguiProvider>
  );
}
