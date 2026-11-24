// The problem editor screen: the code editor, its keyboard rails and the console (action bar, dock, Testcase and Run Result sheet).
import { Logger } from '@lib/logging';
import { toast } from 'sonner-native';

import { useUnsavedChangesRequestStore } from '../state/unsavedChangesRequestStore';
import {
  IconButton,
  KeyboardToolbarAwareScrollView,
  KeyboardToolbarTarget,
  ScreenHeader,
  Spinner,
  Text,
  useKeyboardFrame,
  useNativeHeaderStore,
  usePresetThemeStore,
  View,
} from '@lib/tamagui-design-system';
import type { EditAlternative } from '@lib/voice-edit-dsl';
import { router } from 'expo-router';
import { Mic, Pencil } from '@lib/tamagui-design-system/icons';
import React from 'react';
import { BackHandler, Platform, Pressable, useColorScheme, type LayoutChangeEvent, type ScrollViewInstance } from 'react-native';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';

import { useLeetCodePorts } from '../state/ports';
import { useLeetCodeFlag } from '../state/useLeetCodeFlag';
import {
  EditorInput,
  type EditorInputHandle,
} from './editor/EditorInput';
import type { EditOutcome, EditTarget } from '../domain/editor/voiceEditPlan';
import { useErrorJump } from '../state/run/useErrorJump';
import type { PlanEditor } from '../plan';
import { toEditorLanguage } from '../domain/editor/languageProfile';
import { completionEntriesFor } from '../domain/completionEntries';
import { useAutocompleteSettingStore } from '../state/autocompleteSettingStore';
import { type Selection } from '../domain/editor/types';
import type { BlocksSource } from './blocks/BlockView';
import { EditorHelperRails } from './editor/helperbars/EditorHelperRails';
import { ProblemCatalog } from '../domain/problemCatalog';
import { compactVoiceError } from '../domain/voiceEditIdentity';
import { useVoiceApply } from '../state/useVoiceApply';
import { useVoiceEdit } from '../state/useVoiceEdit';
import { useLeetCodeAuthStore } from '../state/authStore';
import { useCodeHistoryRequestStore } from '../state/codeHistoryRequestStore';
import { useCodeHistoryStore } from '../state/codeHistoryStore';
import { useResetCodeRequestStore } from '../state/resetCodeRequestStore';

import { AlternativePicker } from './AlternativePicker';
import { DiffToggle } from './DiffToggle';
import {
  EDITOR_LANGUAGES,
  LanguagePicker,
  LanguageSheet,
  languageLabel,
  type EditorLanguage,
} from './LanguagePicker';
import { VoiceRecordingToolbar } from './VoiceRecordingToolbar';
import { VoiceReviewSheet } from './VoiceReviewSheet';
import { useEditorAppearance } from './editor/useEditorAppearance';
import { editorLanguageMenu } from './editor-header/editorLanguageMenu';
import { editorHeaderSpec } from './editor-header/editorHeaderSpec';
import { editorSettingsMenu } from './editor-settings/editorSettingsMenu';
import { SymbolBarSheet } from './editor-settings/SymbolBarSheet';
import { railStyleMenu } from './editor-settings/railStyleMenu';
import { useSymbolBarStore } from '../state/editor/symbolBarStore';
import { useEditorPreferencesStore } from '../state/editor/editorPreferencesStore';
import { useConsole } from '../state/console/useConsole';
import { consoleInset, editorZoneMinHeight } from '../domain/console/layout';
import { ConsoleActionBar } from './console/ConsoleActionBar';
import { ConsoleDock } from './console/ConsoleDock';
import { ConsoleFooter } from './console/ConsoleFooter';
import { ConsoleSheet } from './console/ConsoleSheet';
import { PendingOverlay } from './console/PendingOverlay';
import { RunResult } from './console/RunResult';

function pickStarterCode(
  starterCode: Record<string, string> | undefined,
  lang: EditorLanguage
): string {
  if (!starterCode) return '';
  return starterCode[lang] ?? Object.values(starterCode)[0] ?? '';
}

const VOICE_ERROR_AUTO_DISMISS_MS = 8_000;
const iosCanvas = Platform.OS === 'ios';

const subscribeNever = () => () => undefined;
// False while the web export's prerendered page hydrates (it has no problem yet), true from the next render on and always on a phone.
const useHydrated = () => React.useSyncExternalStore(subscribeNever, () => true, () => false);

export function ProblemEditorScreen({
  id,
  seed,
  lang: initialLang,
  onWorkspaceApi,
  active = true,
  onConsoleOpenChange,
  onBlocksSource,
}: {
  id?: string;
  /** Dev bundles only: the code the editor opens on, so a device-lab job can start mid-problem. */
  seed?: string;
  /** Dev bundles only: the language the editor opens in. */
  lang?: EditorLanguage;
  onWorkspaceApi?: (api: PlanEditor) => void;
  /** False while another workspace tab is shown: the console is closed then. */
  active?: boolean;
  onConsoleOpenChange?: (open: boolean) => void;
  /** What the Blocks tab reads the code through: the editor handle, its language and whether its page has hydrated. */
  onBlocksSource?: (source: BlocksSource) => void;
}) {
  const problem = React.useMemo(
    () => (id ? ProblemCatalog.getById(id) : undefined),
    [id]
  );

  const [lang, setLang] = React.useState<EditorLanguage>(initialLang ?? 'python3');
  const editorLanguage = toEditorLanguage(lang) ?? 'python';
  const autocomplete = useAutocompleteSettingStore((s) => s.autocomplete);
  const [editorReady, setEditorReady] = React.useState(false);
  // the suggestions follow the language: the problem's own names from its starter, and the language's keywords
  const completion = React.useMemo(
    () => ({ enabled: autocomplete, entries: problem ? completionEntriesFor(problem, editorLanguage) : [] }),
    [autocomplete, problem, editorLanguage]
  );
  const [editorText, setEditorText] = React.useState(
    () => seed || pickStarterCode(problem?.starterCode, lang)
  );
  const [editorSelection, setEditorSelection] = React.useState<Selection>({
    start: 0,
    end: 0,
  });

  const editorRef = React.useRef<EditorInputHandle>(null);
  const editorFocusedRef = React.useRef(false);
  const refocusAfterConsole = React.useRef(false);
  // The editor's focus lives in a WebView that React Native does not track, so Keyboard.dismiss() and the scroll view's
  // tap-away reach nothing; the editor is told to blur itself.
  const dismissEditorKeyboard = React.useCallback(() => {
    Logger.debug('editor_v2.keyboard_dismiss', { problemId: problem?.id });
    editorRef.current?.blur();
  }, [problem?.id]);
  // the buffer and the language as they are now, for a run that finishes after the person has gone on typing
  const latestBufferRef = React.useRef({ text: editorText, lang });
  React.useEffect(() => {
    latestBufferRef.current = { text: editorText, lang };
  }, [editorText, lang]);
  const { gateway, runner } = useLeetCodePorts();
  const aiEdit = useLeetCodeFlag('aiEdit');
  const scrollRef = React.useRef<ScrollViewInstance>(null);
  const errorJump = useErrorJump(editorRef);
  const scrollToEditor = React.useCallback((line: number) => {
    Logger.info('editor_v2.run_error.scroll_to_editor', { line });
    scrollRef.current?.scrollTo({ y: 0, animated: true });
  }, []);
  const consoleRuntime = {
    runner,
    editor: () => editorRef.current,
    latest: () => latestBufferRef.current,
    dismissKeyboard: dismissEditorKeyboard,
    errorJump,
    onRevealed: scrollToEditor,
    onStale: () => toast('Code changed while it ran', { description: 'Run it again to see its result.' }),
  };
  const consoleState = useConsole(problem, consoleRuntime);
  const { closeConsole, openConsole, resetCases, clearResult: clearConsoleResult, onCodeChanged: onConsoleCodeChanged, pending: consolePending } = consoleState;
  const consoleOpen = consoleState.open;
  const consolePendingRef = React.useRef(false);
  consolePendingRef.current = consolePending !== null;
  const keyboard = useKeyboardFrame();
  const nativeEditorHeader = useNativeHeaderStore((state) => state.enabled) && iosCanvas;
  const [rootHeight, setRootHeight] = React.useState(0);
  React.useEffect(() => {
    onConsoleOpenChange?.(consoleOpen);
  }, [consoleOpen, onConsoleOpenChange]);
  React.useEffect(() => {
    if (!active && !consolePendingRef.current) closeConsole('plan-tab');
  }, [active, closeConsole]);
  React.useEffect(() => {
    if (Platform.OS !== 'android' || !consoleOpen) return undefined;
    const subscription = BackHandler.addEventListener('hardwareBackPress', () => {
      if (!consolePendingRef.current) closeConsole('back');
      return true;
    });
    return () => subscription.remove();
  }, [consoleOpen, closeConsole]);
  const handleOpenConsole = React.useCallback(() => {
    refocusAfterConsole.current = iosCanvas && keyboard.visible && editorFocusedRef.current;
    dismissEditorKeyboard();
    openConsole('testcase');
  }, [dismissEditorKeyboard, keyboard.visible, openConsole]);
  const handleConsoleClosed = React.useCallback(() => {
    closeConsole('sheet');
    if (!refocusAfterConsole.current) return;
    refocusAfterConsole.current = false;
    requestAnimationFrame(() => editorRef.current?.focus());
  }, [closeConsole]);
  const handleResetCases = React.useCallback(() => {
    resetCases();
    toast('Testcases reset');
  }, [resetCases]);
  const hydrated = useHydrated();
  const { run: runConsole } = consoleState;
  const consoleActions = {
    onOpen: handleOpenConsole,
    onReset: handleResetCases,
    onRun: () => void runConsole('run'),
    onSubmit: () => void runConsole('submit'),
    pending: consolePending !== null,
    noCases: hydrated && !consoleState.hasBundled,
  };
  const [diffMode, setDiffMode] = React.useState(false);
  // whether a voice or plan edit has left a mark: the editor holds the mark and the range, the screen only the toggle
  const [hasEdit, setHasEdit] = React.useState(false);
  const isLoggedIn = useLeetCodeAuthStore((s) => s.isLoggedIn);
  const username = useLeetCodeAuthStore((s) => s.username);
  const clearCredentials = useLeetCodeAuthStore((s) => s.clearCredentials);
  const recordSnapshot = useCodeHistoryStore((s) => s.record);
  const historyCount = useCodeHistoryStore(
    (s) => s.snapshots.filter((snap) => snap.problemId === problem?.id).length
  );

  // Snapshot the buffer that is ABOUT to be replaced, not the replacement --
  // history is only worth anything if it holds what you are losing.
  const snapshot = React.useCallback(
    (reason: Parameters<typeof recordSnapshot>[0]['reason'], code: string = editorText) => {
      if (!problem) return;
      recordSnapshot({
        problemId: problem.id,
        language: lang,
        code,
        reason,
      });
    },
    [problem, lang, editorText, recordSnapshot]
  );

  // The editor as a voice edit sees it: read for the plan and applied at the revision the plan was made for.
  const editorTarget = React.useMemo<EditTarget>(
    () => ({
      basis: () => editorRef.current?.basis() ?? Promise.resolve(null),
      applyEdit: (edit) => editorRef.current?.applyEdit(edit) ?? Promise.resolve({ ok: false, reason: 'not-ready', code: 'not-ready' }),
    }),
    []
  );
  const voiceEdit = useVoiceEdit(editorText, lang, `${id ?? 'problem'}.m4a`, {
    problemId: problem?.id ?? null,
    readBasis: editorTarget.basis,
  });

  // "Dirty" is measured against the starter code for the CURRENT language, not
  // against a saved copy -- there is no saved copy. Switching language resets
  // the buffer to that language's starter, so the comparison stays honest.
  const isDirty = React.useMemo(
    () => editorText !== pickStarterCode(problem?.starterCode, lang),
    [editorText, problem?.starterCode, lang]
  );
  const requestUnsavedChanges = useUnsavedChangesRequestStore((s) => s.request);
  const [languageSheetOpen, setLanguageSheetOpen] = React.useState(false);

  // Only a language the problem has a starter for: without one the buffer would be another language's code under this one's name
  const availableLanguages = React.useMemo(
    () => EDITOR_LANGUAGES.filter((candidate) => problem?.starterCode?.[candidate] !== undefined),
    [problem?.starterCode]
  );

  const handleLanguageChange = React.useCallback(
    (nextLang: EditorLanguage) => {
      if (!availableLanguages.includes(nextLang)) {
        Logger.warn('editor_v2.language_change.refused', { to: nextLang, problemId: problem?.id });
        return;
      }
      Logger.info('editor_v2.language_change', {
        from: lang,
        to: nextLang,
        problemId: problem?.id,
      });
      // A language switch discards the whole buffer for the new language's
      // starter code. That was silent data loss before this snapshot.
      snapshot('language-switch');
      setLang(nextLang);
      setEditorText(pickStarterCode(problem?.starterCode, nextLang));
      clearConsoleResult();
      runner.warmUp?.(nextLang);
    },
    [availableLanguages, lang, problem?.id, problem?.starterCode, snapshot, clearConsoleResult, runner]
  );

  const handleFormat = React.useCallback(async () => {
    try {
      const formatted = await gateway.format({ lang, code: editorText });
      snapshot('format');
      setEditorText(formatted);
      Logger.info('editor_v2.format.result', {
        length: formatted.length,
        problemId: problem?.id,
      });
      toast.success('Formatted', {
        description: `${formatted.split('\n').length} lines · saved to history`,
      });
    } catch (err) {
      Logger.withError(err)
        .withMetadata({ problemId: problem?.id })
        .error('editor_v2.format.error');
      toast.error('Format failed', {
        description:
          err instanceof Error ? err.message : 'The formatter did not respond.',
      });
    }
  }, [editorText, gateway, problem, lang, snapshot]);

  const handleEditorChange = React.useCallback((next: string) => {
    setEditorText(next);
    onConsoleCodeChanged(next);
  }, [onConsoleCodeChanged]);

  // React follows the page's contentChanged for the text, and the editor marks what the edit changed as it applies it; the screen only shows the toggle.
  const showEdit = React.useCallback((outcome: EditOutcome) => {
    if (!outcome.ok || !outcome.landed) return;
    setHasEdit(true);
    setDiffMode(true);
  }, []);
  const { apply: applyVoice, applyAt } = useVoiceApply({ target: editorTarget, voiceEdit, snapshot, problemId: problem?.id });
  const handleVoiceApply = React.useCallback(
    (alt: EditAlternative) => {
      void applyVoice(alt).then(showEdit);
    },
    [applyVoice, showEdit]
  );

  // Only once the state that carries the plan's basis is on screen: a render between the plan returning and that state landing
  // (a keystroke's echo, say) would otherwise apply it with no basis.
  React.useEffect(() => {
    if (voiceEdit.state.phase !== 'applied') return;
    const alt = voiceEdit.consumeAutoApply();
    if (alt) {
      handleVoiceApply(alt);
    }
  }, [voiceEdit, handleVoiceApply]);

  // The plan tab reads the editor and applies at the revision it read, through the same call as a voice apply (minus the voice
  // bookkeeping); re-register when they move.
  React.useEffect(() => {
    if (!onWorkspaceApi) return;
    onWorkspaceApi({
      language: lang,
      basis: editorTarget.basis,
      applyEdit: async (alt, basis) => {
        const outcome = await applyAt(alt, basis, 'plan-edit');
        showEdit(outcome);
        return outcome;
      },
    });
  }, [lang, onWorkspaceApi, editorTarget, applyAt, showEdit]);

  React.useEffect(() => {
    onBlocksSource?.({ editorRef, language: editorLanguage, ready: editorReady });
  }, [onBlocksSource, editorLanguage, editorReady]);

  const handleDiffToggle = React.useCallback(
    (enabled: boolean) => {
      Logger.info('editor_v2.diff_toggle', { enabled, problemId: problem?.id });
      setDiffMode(enabled);
      editorRef.current?.setEditMarkHidden(!enabled);
    },
    [problem?.id]
  );

  const handleVoiceStart = React.useCallback(async () => {
    Logger.info('editor_v2.voice_start', { problemId: problem?.id });
    closeConsole('voice');
    await voiceEdit.start();
  }, [voiceEdit, problem?.id, closeConsole]);

  const handleTextEditStart = React.useCallback(() => {
    Logger.info('editor_v2.text_edit_start', { problemId: problem?.id });
    closeConsole('voice');
    voiceEdit.startTextEdit();
  }, [voiceEdit, problem?.id, closeConsole]);

  const handleVoiceStop = React.useCallback(async () => {
    Logger.info('editor_v2.voice_stop', { problemId: problem?.id });
    await voiceEdit.stop();
  }, [voiceEdit, problem?.id]);

  const handleVoiceCancel = React.useCallback(() => {
    Logger.info('editor_v2.voice_cancel', { problemId: problem?.id });
    voiceEdit.cancel();
  }, [voiceEdit, problem?.id]);

  const handleVoiceDismiss = React.useCallback(() => {
    Logger.info('editor_v2.voice_dismiss', { problemId: problem?.id });
    voiceEdit.dismiss();
  }, [voiceEdit, problem?.id]);

  // The banner sits over the record button, so an error that stays up
  // blocks the next attempt. It clears itself after a while, and a tap
  // clears it now.
  React.useEffect(() => {
    if (voiceEdit.state.phase !== 'error') return undefined;
    const timer = setTimeout(handleVoiceDismiss, VOICE_ERROR_AUTO_DISMISS_MS);
    return () => clearTimeout(timer);
  }, [voiceEdit.state.phase, handleVoiceDismiss]);

  const handleVoiceReRecord = React.useCallback(() => {
    Logger.info('editor_v2.voice_re_record', { problemId: problem?.id });
    voiceEdit.reRecord();
    void handleVoiceStart();
  }, [voiceEdit, problem?.id, handleVoiceStart]);

  const handleVoiceConfirmSend = React.useCallback(() => {
    Logger.info('editor_v2.voice_confirm_send', { problemId: problem?.id });
    void voiceEdit.confirmSend();
  }, [voiceEdit, problem?.id]);

  const problemSubtitle = problem
    ? `${problem.category} · ${problem.difficulty[0]?.toUpperCase()}${problem.difficulty.slice(1)}`
    : undefined;

  const handleResetCode = React.useCallback(() => {
    if (!problem) return;
    Logger.info('editor_v2.reset_code.request', { problemId: problem.id });
    useResetCodeRequestStore.getState().request({
      problemId: problem.id,
      onConfirm: () => {
        Logger.info('editor_v2.reset_code.confirm', {
          problemId: problem.id,
        });
        // Reset is the most destructive path in the screen and used to be
        // genuinely irreversible -- the dialog said so. Snapshot first.
        snapshot('reset');
        setEditorText(pickStarterCode(problem.starterCode, lang));
        clearConsoleResult();
        // The edit mark belongs to the code that was just discarded.
        setHasEdit(false);
        setDiffMode(false);
        editorRef.current?.clearEditMark();
      },
      onCancel: () => {
        Logger.info('editor_v2.reset_code.cancel', { problemId: problem.id });
      },
    });
    router.push(`/leetcode/${problem.id}/reset-code`);
  }, [problem, lang, snapshot, clearConsoleResult]);

  const fontSize = useEditorPreferencesStore((s) => s.fontSize);
  const tabSize = useEditorPreferencesStore((s) => s.tabSize);
  const wrap = useEditorPreferencesStore((s) => s.wrap);
  const whitespace = useEditorPreferencesStore((s) => s.whitespace);
  const setFontSize = useEditorPreferencesStore((s) => s.setFontSize);
  const setTabSize = useEditorPreferencesStore((s) => s.setTabSize);
  const setWrap = useEditorPreferencesStore((s) => s.setWrap);
  const setWhitespace = useEditorPreferencesStore((s) => s.setWhitespace);
  const railStyle = useSymbolBarStore((state) => state.railStyle);
  const setRailStyle = useSymbolBarStore((state) => state.setRailStyle);
  const [symbolBarOpen, setSymbolBarOpen] = React.useState(false);
  const refocusAfterSheetRef = React.useRef(false);
  const handleSymbolBarOpen = React.useCallback(() => {
    Logger.info('editor_v2.symbol_bar.open', { problemId: problem?.id });
    refocusAfterSheetRef.current = editorFocusedRef.current;
    dismissEditorKeyboard();
    setSymbolBarOpen(true);
  }, [dismissEditorKeyboard, problem?.id]);
  const restoreAfterSymbolBar = React.useCallback(() => {
    if (!refocusAfterSheetRef.current) return;
    refocusAfterSheetRef.current = false;
    editorRef.current?.focus();
  }, []);
  const handleSymbolBarOpenChange = React.useCallback((open: boolean) => {
    setSymbolBarOpen(open);
    if (!open && !iosCanvas) restoreAfterSymbolBar();
  }, [restoreAfterSymbolBar]);

  const handleBack = () => {
    Logger.info('editor.back', {
      problemId: problem?.id,
      dirty: isDirty,
    });
    if (!isDirty) {
      router.back();
      return;
    }
    requestUnsavedChanges({
      problemId: problem?.id ?? '',
      onDiscard: () => {
        // Snapshot BEFORE leaving: the buffer is useState seeded
        // from starter code and persisted nowhere, so without this
        // "discard" would be irreversible.
        snapshot('navigate-away');
        router.back();
      },
      onCancel: () => {},
    });
    router.push(`/leetcode/${id ?? ''}/unsaved`);
  };

  const handleInfo = () => {
    Logger.info('editor_v2.info_tap', { problemId: problem?.id });
    dismissEditorKeyboard();
    if (problem) router.push(`/leetcode/${problem.id}/description`);
  };

  const handleHistory = () => {
    Logger.info('editor_v2.history_tap', { problemId: problem?.id, count: historyCount });
    dismissEditorKeyboard();
    if (!problem) return;
    // Register the restore target BEFORE pushing: the sheet
    // is its own route, so it shares no React tree with this
    // screen and cannot reach setEditorText directly.
    useCodeHistoryRequestStore.getState().request({
      problemId: problem.id,
      onRestore: (code, language) => {
        snapshot('reset');
        setEditorText(code);
        editorRef.current?.setTextAndSelection(code, { start: code.length, end: code.length });
        if (language !== lang) {
          setLang(language as EditorLanguage);
        }
      },
    });
    router.push(`/leetcode/${problem.id}/history`);
  };

  const handleLogin = () => {
    if (isLoggedIn) {
      Logger.info('editor_v2.logout_tap', { problemId: problem?.id });
      clearCredentials();
      return;
    }
    Logger.info('editor_v2.login_tap', { problemId: problem?.id });
    router.push('/leetcode/login');
  };

  // Off by default, the code stays wired: see domain/featureFlags.ts.
  const editButtons = (
    <>
        <IconButton
          testID="ProblemEditorTextEditStartButton"
          icon={Pencil}
          variant="default"
          aria-label="Type an edit instruction"
          pressLog="off"
          onPress={() => {
            Logger.info('editor_v2.text_edit_button.tap', {
              problemId: problem?.id,
            });
            dismissEditorKeyboard();
            handleTextEditStart();
          }}
        />
        <IconButton
          testID="ProblemEditorVoiceStartButton"
          icon={Mic}
          variant="default"
          aria-label="Start voice edit"
          pressLog="off"
          onPress={() => {
            Logger.info('editor_v2.voice_start_button.tap', {
              problemId: problem?.id,
            });
            dismissEditorKeyboard();
            void handleVoiceStart();
          }}
        />
    </>
  );

  const { backgroundColor, palette } = useEditorAppearance(iosCanvas);
  const appThemeMode = usePresetThemeStore((state) => state.themeMode);
  const systemScheme = useColorScheme();
  const appScheme = appThemeMode === 'system' ? systemScheme ?? 'light' : appThemeMode;
  const canvasBackground = iosCanvas && appScheme === palette.appearance ? backgroundColor : undefined;
  const darkCanvas = iosCanvas && appScheme === 'dark' && palette.appearance === 'dark';
  const headerSpec = editorHeaderSpec({
    title: problem?.title ?? 'Problem',
    subtitle: problemSubtitle,
    historyCount,
    isLoggedIn,
    username,
    busy: consolePending !== null,
    nativeWorkspace: nativeEditorHeader ? {
      dirty: isDirty,
      backgroundColor: canvasBackground,
      railStyle: railStyleMenu(railStyle, setRailStyle),
      language: editorLanguageMenu(lang, availableLanguages, handleLanguageChange),
    } : undefined,
    settings: editorSettingsMenu(
          { fontSize, tabSize, wrap, whitespace },
          {
            onFontSize: setFontSize,
            onTabSize: setTabSize,
            onWrap: setWrap,
            onWhitespace: setWhitespace,
            onSymbolBar: handleSymbolBarOpen,
            onFormat: () => {
              Logger.info('editor_v2.format.tap', { problemId: problem?.id, from: 'settings_menu' });
              void handleFormat();
            },
          },
          nativeEditorHeader ? undefined : languageLabel(lang)
        ),
    onBack: handleBack,
    onInfo: handleInfo,
    onUndo: iosCanvas && isDirty ? () => editorRef.current?.undo() : undefined,
    onHistory: handleHistory,
    onLogin: handleLogin,
    onResetCode: handleResetCode,
  });

  return (
    <View
      flex={1}
      bg="$background"
      style={{ backgroundColor: canvasBackground }}
      testID="ProblemEditorScreen"
      onLayout={(event: LayoutChangeEvent) => setRootHeight(Math.round(event.nativeEvent.layout.height))}
    >
      <ScreenHeader {...headerSpec} glass placement="navigator" />
      <KeyboardToolbarTarget
        testID="ProblemEditorToolbarTarget"
        toolbarTestID="EditorHelperRails"
        toolbar={() => (
          <EditorHelperRails
            editorRef={editorRef}
            source={editorText}
            selection={editorSelection}
            language={editorLanguage}
            onConfigure={() => {
              Logger.info('editor_v2.toolbar_configure', {
                problemId: problem?.id,
              });
              router.push('/leetcode/toolbar');
            }}
            onDismissKeyboard={dismissEditorKeyboard}
            compactSymbols={iosCanvas && !aiEdit}
            symbolBarBackground={darkCanvas ? canvasBackground : undefined}
            topRow={<ConsoleActionBar {...consoleActions} canvas={darkCanvas} backgroundColor={darkCanvas ? canvasBackground : undefined} />}
          />
        )}
      >
        <KeyboardToolbarAwareScrollView
          ref={scrollRef}
          testID="ProblemEditorScroll"
          keyboardShouldPersistTaps="handled"
          {...(consoleOpen && rootHeight > 0 ? { bottomInset: consoleInset(rootHeight, iosCanvas) } : {})}
        >
          {/* A tap on the page outside the editor lowers its keyboard. The editor sits beside that Pressable and not
              inside it: a view that claims the touch responder above the WebView makes Android cancel the WebView's
              own touches (a long press, a caret drag) once the finger moves. */}
          <YStack style={{ flexGrow: 1 }}>
            {/* Editor (CodeMirror WebView) — fixed min-h so the ScrollView has
                something concrete to scroll past when the keyboard shrinks the
                visible area. */}
            <YStack
              flex={1}
              minH={editorZoneMinHeight(consoleOpen)}
              px={iosCanvas ? 0 : "$3"}
              py={iosCanvas ? 0 : "$2"}
              testID="ProblemEditorEditorZone"
            >
              <EditorInput
                ref={editorRef}
                testID="ProblemEditorInput"
                appearance={iosCanvas ? 'canvas' : 'panel'}
                language={editorLanguage}
                completion={completion}
                value={editorText}
                onChangeText={handleEditorChange}
                onSelectionChange={setEditorSelection}
                onFocusChange={(focused) => {
                  editorFocusedRef.current = focused;
                  if (focused) closeConsole('editor-focus');
                }}
                onHydratedChange={(hydrated) => {
                  setEditorReady(hydrated);
                  // the page may have been dropped while idle: bring it back while the person reads the problem, not at Run
                  if (hydrated) runner.warmUp?.(lang);
                }}
              />
            </YStack>

            <Pressable
              accessible={false}
              onPress={dismissEditorKeyboard}
              testID="ProblemEditorTapAway"
            >
            <XStack
              items="center"
              justify="space-between"
              px="$3"
              pt="$2"
              testID="ProblemEditorDiffRow"
            >
              {!nativeEditorHeader ? <LanguagePicker
                value={lang}
                onOpen={() => setLanguageSheetOpen(true)}
                // exactly one: the page the web export prerenders has no problem yet (no languages), and a trigger disabled there
                // would not match the enabled one a problem with two languages hydrates into
                disabled={availableLanguages.length === 1}
                testID="ProblemEditorLanguagePicker"
              /> : null}
              {hasEdit ? (
                <DiffToggle
                  enabled={diffMode}
                  onToggle={handleDiffToggle}
                  testID="ProblemEditorDiffToggle"
                />
              ) : null}
              {aiEdit ? <XStack gap="$2">{editButtons}</XStack> : null}
            </XStack>

            </Pressable>
          </YStack>
        </KeyboardToolbarAwareScrollView>
        {!consoleOpen && !keyboard.visible ? <ConsoleDock {...consoleActions} canvas={darkCanvas} backgroundColor={canvasBackground} /> : null}
      </KeyboardToolbarTarget>
      {problem ? (
        <ConsoleSheet
          problem={problem}
          console={consoleState}
          fieldError={consoleState.fieldError}
          result={<RunResult outcome={consoleState.outcome} actions={{ onShowLine: () => void consoleState.showLine(), canOpenCase: (label) => consoleState.caseIdOf(label) !== null, onOpenCase: consoleState.openCase }} />}
          footer={<ConsoleFooter {...consoleActions} canvas={darkCanvas} />}
          onSheetClosed={handleConsoleClosed}
          editorHeightBelowNativeHeader={nativeEditorHeader ? rootHeight : undefined}
        />
      ) : null}
      <LanguageSheet
        open={languageSheetOpen}
        onOpenChange={setLanguageSheetOpen}
        value={lang}
        onChange={handleLanguageChange}
        languages={availableLanguages}
        testID="ProblemEditorLanguagePicker"
      />
      <SymbolBarSheet
        open={symbolBarOpen}
        onOpenChange={handleSymbolBarOpenChange}
        onDismiss={restoreAfterSymbolBar}
        showRailStyle={!nativeEditorHeader}
      />
      {consolePending ? <PendingOverlay /> : null}
      {voiceEdit.state.phase === 'planning' ? (
        <XStack
          position="absolute"
          b={80}
          l="$4"
          r="$4"
          p="$3"
          gap="$2"
          rounded="$3"
          borderWidth={1}
          borderColor="$borderColor"
          bg="$background"
          items="center"
          testID="ProblemEditorVoiceProcessing"
        >
          <Spinner size="small" testID="ProblemEditorVoiceProcessingSpinner" />
          <Text variant="small" testID="ProblemEditorVoiceProcessingText">
            Thinking…
          </Text>
        </XStack>
      ) : null}
      {voiceEdit.state.phase === 'transcribing' ||
      voiceEdit.state.phase === 'reviewing' ? (
        <VoiceReviewSheet
          mode={voiceEdit.state.source === 'text' ? 'text' : 'voice'}
          recordingUri={voiceEdit.state.recordingUri}
          transcript={voiceEdit.state.transcript}
          onTranscriptChange={voiceEdit.updateTranscript}
          onReRecord={handleVoiceReRecord}
          onCancel={handleVoiceCancel}
          onSend={handleVoiceConfirmSend}
          testID="ProblemEditorVoiceReviewSheet"
        />
      ) : null}
      {voiceEdit.state.phase === 'choosing' ? (
        <AlternativePicker
          transcript={voiceEdit.state.transcript ?? ''}
          alternatives={voiceEdit.state.alternatives}
          onSelect={handleVoiceApply}
          onDismiss={handleVoiceDismiss}
          testID="ProblemEditorAlternativePicker"
        />
      ) : null}
      {voiceEdit.state.phase === 'error' ? (
        <YStack
          position="absolute"
          b={80}
          l="$4"
          r="$4"
          p="$3"
          rounded="$3"
          borderWidth={1}
          borderColor="$red9"
          bg="$red2"
          pressStyle={{ opacity: 0.7 }}
          onPress={handleVoiceDismiss}
          role="button"
          aria-label="Dismiss voice edit error"
          testID="ProblemEditorVoiceErrorBanner"
        >
          <Text
            variant="small"
            color="$red10"
            testID="ProblemEditorVoiceErrorText"
          >
            {compactVoiceError(voiceEdit.state.error)}
          </Text>
          <Text variant="muted" testID="ProblemEditorVoiceErrorHint">
            Tap to dismiss
          </Text>
        </YStack>
      ) : null}
      {voiceEdit.state.phase === 'recording' ? (
        <YStack
          position="absolute"
          b={0}
          l={0}
          r={0}
          px="$3"
          pt="$2"
          pb="$4"
          borderTopWidth={1}
          borderColor="$borderColor"
          bg="$background"
          testID="ProblemEditorVoiceOverlay"
        >
          <VoiceRecordingToolbar
            isRecording
            durationMs={voiceEdit.durationMs}
            amplitudes={voiceEdit.amplitudes}
            onCancel={handleVoiceCancel}
            onSend={handleVoiceStop}
            testID="ProblemEditorVoiceToolbar"
          />
        </YStack>
      ) : null}
    </View>
  );
}
