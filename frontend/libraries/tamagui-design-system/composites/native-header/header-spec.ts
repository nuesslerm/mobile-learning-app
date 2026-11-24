import type { LucideIcon } from 'lucide-react-native';

import type { HeaderMenuSpec, SfSymbolName } from './header-menu-spec';

export type HeaderActionSpec = {
  icon: LucideIcon;
  /** The native bar draws this glyph; without it the item shows its accessibility label as text. */
  sfSymbol?: SfSymbolName;
  accessibilityLabel: string;
  onPress: () => void;
  testID: string;
};

export type HeaderStatusSpec = {
  label: string;
  icon: LucideIcon;
  sfSymbol?: SfSymbolName;
  testID: string;
  menu?: Pick<HeaderMenuSpec, 'accessibilityLabel' | 'items'>;
};

export type HeaderGroupItem = HeaderActionSpec | HeaderMenuSpec;

/** Controls that share one piece of glass, in the order they appear. */
export type HeaderGroup = HeaderGroupItem[];

/** One description of a top bar, drawn by the classic renderer or by the native one. Today's ScreenHeaderProps is assignable to it. */
export type ScreenHeaderSpec = {
  title: string;
  /** Secondary line under the title. Keep it to context, not restatement. */
  subtitle?: string;
  nativeTitle?: 'capsule' | 'hidden';
  leadingStatus?: HeaderStatusSpec;
  onBack?: () => void;
  backAccessibilityLabel?: string;
  /** Today's one pill: the actions, then the menu last. Ignored when `groups` is given. */
  actions?: HeaderActionSpec[];
  menu?: HeaderMenuSpec;
  /** The pieces of glass left to right; each group shares one. */
  groups?: HeaderGroup[];
  /** A run is in flight: a spinner on a piece of glass of its own. */
  busy?: boolean;
  /** Native bar only: the content scrolls under the glass instead of starting below an opaque bar. Ignored by the classic header. */
  transparent?: boolean;
  nativeBackgroundColor?: string;
  nativeGlassMenuGroups?: boolean;
  testID: string;
};

/** What a screen reader hears from the spinner of a busy bar. */
export const HEADER_BUSY_LABEL = 'Working';

export function isMenuSpec(item: HeaderGroupItem): item is HeaderMenuSpec {
  return 'items' in item;
}

export function headerGroups(spec: ScreenHeaderSpec): HeaderGroup[] {
  const groups = spec.groups ?? [
    [...(spec.actions ?? []), ...(spec.menu ? [spec.menu] : [])],
  ];
  return groups.filter((group) => group.length > 0);
}
