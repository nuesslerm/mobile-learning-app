import type { NativeStackNavigationOptions } from 'expo-router';
import type { ReactElement, ReactNode } from 'react';

import { nativeHeaderActive, type HeaderEnv } from './header-env';
import { menuSignature } from './header-menu-spec';
import { headerGroups, isMenuSpec, type ScreenHeaderSpec } from './header-spec';
import {
  buildNativeHeaderItems,
  type NativeHeaderFactories,
} from './native-header-items';

/** The height of the native bar below the status bar; a constant because the bar reports none. */
export const NATIVE_BAR_CONTENT_HEIGHT = 44;

const CONTROL_WIDTH = 44;
const CONTROL_GAP = 8;
const BAR_MARGIN = 16;
const MIN_TITLE_WIDTH = 96;

export type HeaderControlCounts = { left: number; right: number };

export function countHeaderControls(
  spec: ScreenHeaderSpec
): HeaderControlCounts {
  const grouped = headerGroups(spec).reduce(
    (sum, group) => sum + group.length,
    0
  );
  return { left: spec.onBack ? 1 : 0, right: grouped + (spec.busy ? 1 : 0) };
}

function flank(count: number): number {
  return count === 0
    ? 0
    : BAR_MARGIN + count * CONTROL_WIDTH + (count - 1) * CONTROL_GAP;
}

/** The widest the title capsule may be: both flanks take the room of the busier side, so the title stays centred. */
export function titleMaxWidth(
  windowWidth: number,
  controls: HeaderControlCounts
): number {
  const room =
    windowWidth - 2 * Math.max(flank(controls.left), flank(controls.right));
  return Math.max(MIN_TITLE_WIDTH, Math.floor(room));
}

export type HeaderTitleProps = {
  title: string;
  subtitle?: string;
  maxWidth: number;
};

/** What the options builder cannot make without react-native: the caller hands each one in. */
export type HeaderRenderers = NativeHeaderFactories & {
  /** Today's Tamagui header, for every environment where the native bar does not draw. */
  classic: (spec: ScreenHeaderSpec) => ReactNode;
  /** The glass capsule with the title and subtitle. */
  title: (props: HeaderTitleProps) => ReactElement;
};

export type HeaderOptionsInput = {
  spec: ScreenHeaderSpec;
  env: HeaderEnv;
  /** The scheme the app resolved: the native chrome follows it for this bar. */
  scheme: 'light' | 'dark';
  theme: { background: string; tint: string };
  windowWidth: number;
  renderers: HeaderRenderers;
};

/**
 * The Stack.Screen options of a top bar. The native branch declares an opaque bar (the screen still starts below it) with react-native-screens header
 * items; everything else gets today's `header` function and none of the unstable_ keys, which are written in this folder only so a rename is one place.
 */
export function buildScreenHeaderOptions({
  spec,
  env,
  scheme,
  theme,
  windowWidth,
  renderers,
}: HeaderOptionsInput): NativeStackNavigationOptions {
  if (!nativeHeaderActive(env))
    return { header: () => renderers.classic(spec) };

  const items = buildNativeHeaderItems(spec, renderers);
  const maxWidth = titleMaxWidth(windowWidth, countHeaderControls(spec));
  return {
    headerShown: true,
    headerTransparent: false,
    headerBackVisible: false,
    headerShadowVisible: false,
    headerStyle: { backgroundColor: theme.background },
    headerTintColor: theme.tint,
    headerUserInterfaceStyle: scheme,
    title: spec.title,
    headerTitle: () =>
      renderers.title({ title: spec.title, subtitle: spec.subtitle, maxWidth }),
    unstable_headerLeftItems: () => items.left,
    unstable_headerRightItems: () => items.right,
  };
}

/** What a bar shows, without its handlers: equal for the same data under new closure identities, so a native bar (and an open menu) is rebuilt only when it displays something new. */
export function headerSignature(spec: ScreenHeaderSpec): string {
  return JSON.stringify({
    id: spec.testID,
    title: spec.title,
    subtitle: spec.subtitle,
    back: spec.onBack !== undefined,
    backLabel: spec.backAccessibilityLabel,
    busy: spec.busy === true,
    groups: headerGroups(spec).map((group) =>
      group.map((member) =>
        isMenuSpec(member)
          ? {
              menu: member.testID,
              label: member.label,
              a11y: member.accessibilityLabel,
              sf: member.sfSymbol,
              title: member.title,
              items: menuSignature(member.items),
            }
          : {
              action: member.testID,
              a11y: member.accessibilityLabel,
              sf: member.sfSymbol,
            }
      )
    ),
  });
}
