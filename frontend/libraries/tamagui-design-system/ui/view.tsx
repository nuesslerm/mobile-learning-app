import type { ReactNode } from 'react';

import { YStack, type YStackProps } from '../primitives';

export type ViewProps = YStackProps & { children?: ReactNode };

/**
 * The layout primitive.
 *
 * The shapes diverge deliberately: app #1's View takes a NativeWind
 * `className`, this one takes Tamagui style props. There is no adapter --
 * accepting `className` here would silently drop every one of them, since
 * Tamagui has no such prop. Callsites are rewritten, not bridged.
 */
// Tamagui v2 exports no bare `Stack`; YStack is the column-default box,
// which is what app #1's View behaves like (RN's flexDirection default).
export const View = YStack;
