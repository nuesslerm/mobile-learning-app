import type { HeaderMenuSpec, MenuNode, ScreenHeaderSpec } from '@lib/tamagui-design-system';
import { Cloud, History, Info, LogIn, LogOut, MoreHorizontal, RotateCcw } from '@lib/tamagui-design-system/icons';

import type { DraftStatus } from '../../state/editor/useCodeDrafts';

export const EDITOR_HEADER_IDS = {
  header: 'ProblemEditorHeader',
  info: 'ProblemEditorInfoButton',
  menu: 'ProblemEditorMenuButton',
  history: 'ProblemEditorMenuHistory',
  login: 'ProblemEditorMenuLogin',
  resetCode: 'ProblemEditorMenuResetCode',
  undo: 'ProblemEditorUndo',
} as const;

const SAVE_LABELS: Record<DraftStatus, string> = { saved: 'Saved', saving: 'Saving…', unsaved: 'Unsaved' };

export type EditorHeaderInput = {
  title: string;
  subtitle?: string;
  historyCount: number;
  isLoggedIn: boolean;
  username: string | null;
  /** A run is in flight; shown as a spinner before the groups. */
  busy: boolean;
  /** The editor settings menu, its own piece of glass. */
  settings: HeaderMenuSpec;
  nativeWorkspace?: { language: HeaderMenuSpec; saveStatus: DraftStatus; backgroundColor?: string; railStyle?: MenuNode };
  onBack: () => void;
  onInfo: () => void;
  onUndo?: () => void;
  onHistory: () => void;
  onLogin: () => void;
  /** Given only when the console replaces the Reset button of the classic rows. */
  onResetCode?: () => void;
};

function moreMenu(input: EditorHeaderInput): HeaderMenuSpec {
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
  const resetCode: MenuNode[] = input.onResetCode ? [{ icon: RotateCcw, label: 'Reset code', onPress: input.onResetCode, testID: EDITOR_HEADER_IDS.resetCode }] : [];
  return {
    icon: MoreHorizontal,
    sfSymbol: 'ellipsis',
    accessibilityLabel: 'More actions',
    testID: EDITOR_HEADER_IDS.menu,
    items: [history, ...resetCode, login],
  };
}

/** The editor's top bar as data: the design system draws it as the system header on iOS and the app's own header elsewhere. */
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
  const workspaceActions = input.onUndo ? [[{
    icon: RotateCcw,
    sfSymbol: 'arrow.uturn.backward' as const,
    accessibilityLabel: 'Undo',
    testID: EDITOR_HEADER_IDS.undo,
    onPress: input.onUndo,
  }]] : [];
  const native = input.nativeWorkspace;
  const more = moreMenu(input);
  return {
    ...base,
    busy: input.busy,
    ...(native ? {
      nativeTitle: 'hidden' as const,
      nativeGlassMenuGroups: true,
      nativeBackgroundColor: native.backgroundColor,
      leadingStatus: {
        label: SAVE_LABELS[native.saveStatus],
        icon: Cloud,
        sfSymbol: 'cloud',
        testID: 'ProblemEditorSaveStatus',
        menu: {
          accessibilityLabel: 'Problem actions',
          items: [{ ...info, label: 'Problem description' }, ...more.items, ...(native.railStyle ? [native.railStyle] : [])],
        },
      },
    } : {}),
    groups: [...workspaceActions, ...(native ? [[native.language, input.settings]] : [[input.settings], [info, more]])],
  };
}
