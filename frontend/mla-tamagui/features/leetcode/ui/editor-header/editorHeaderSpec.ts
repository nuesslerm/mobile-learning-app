import type { HeaderMenuSpec, MenuNode, ScreenHeaderSpec } from '@lib/tamagui-design-system';
import { History, Info, LogIn, LogOut, MoreHorizontal, Wand2 } from '@lib/tamagui-design-system/icons';

export const EDITOR_HEADER_IDS = {
  header: 'ProblemEditorHeader',
  info: 'ProblemEditorInfoButton',
  menu: 'ProblemEditorMenuButton',
  format: 'ProblemEditorMenuFormat',
  history: 'ProblemEditorMenuHistory',
  login: 'ProblemEditorMenuLogin',
} as const;

export type EditorHeaderInput = {
  title: string;
  subtitle?: string;
  historyCount: number;
  isLoggedIn: boolean;
  username: string | null;
  /** A run is in flight; shown as a spinner only in the glass layout. */
  busy: boolean;
  /** The glass layout: busy, the settings pill and the grouped Info and More pill. Off, the spec is the classic one. */
  glass: boolean;
  /** The editor settings menu, its own piece of glass in the glass layout. */
  settings?: HeaderMenuSpec;
  onBack: () => void;
  onInfo: () => void;
  onFormat: () => void;
  onHistory: () => void;
  onLogin: () => void;
};

function moreMenu(input: EditorHeaderInput): HeaderMenuSpec {
  const format: MenuNode = { icon: Wand2, label: 'Format', onPress: input.onFormat, testID: EDITOR_HEADER_IDS.format };
  const history: MenuNode = {
    icon: History,
    label: 'Version history',
    detail: input.historyCount > 0 ? `${input.historyCount} saved` : 'No versions yet',
    onPress: input.onHistory,
    testID: EDITOR_HEADER_IDS.history,
  };
  const login: MenuNode = {
    icon: input.isLoggedIn ? LogOut : LogIn,
    label: input.isLoggedIn ? 'Log out' : 'Login',
    detail: input.isLoggedIn ? (input.username ?? undefined) : 'LeetCode',
    onPress: input.onLogin,
    testID: EDITOR_HEADER_IDS.login,
  };
  return {
    icon: MoreHorizontal,
    sfSymbol: 'ellipsis',
    accessibilityLabel: 'More actions',
    testID: EDITOR_HEADER_IDS.menu,
    items: input.glass ? [history, login] : [format, history, login],
  };
}

/** The editor's top bar as data: the design system draws it as the classic header or the native glass bar. */
export function editorHeaderSpec(input: EditorHeaderInput): ScreenHeaderSpec {
  const base = {
    testID: EDITOR_HEADER_IDS.header,
    title: input.title,
    subtitle: input.subtitle,
    backAccessibilityLabel: 'Back to the problem list',
    onBack: input.onBack,
  };
  const info = {
    icon: Info,
    sfSymbol: 'info.circle' as const,
    accessibilityLabel: 'Problem description',
    testID: EDITOR_HEADER_IDS.info,
    onPress: input.onInfo,
  };
  const more = moreMenu(input);
  if (!input.glass) return { ...base, actions: [info], menu: more };
  return { ...base, busy: input.busy, groups: [input.settings ? [input.settings] : [], [info, more]] };
}
