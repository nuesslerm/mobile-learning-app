// The problem editor screen.
//
// Layout (top → bottom, inside a keyboard-aware vertically scrolling page):
//   • <Stack.Screen> header: problem title + info IconButton (right)
//   • CodeMirror WebView editor (fixed min-h, fills top-to-bottom)
//   • Action row: Format | Run Local | Run (mirrors V1's action row,
//     minus the Submit button which the user explicitly dropped)
//   • Custom test input (fixed min-h)
//
// The whole page is a `KeyboardToolbarAwareScrollView` so the editor +
// test input remain reachable when the soft keyboard covers the lower
// half of the screen; the scroll view's `contentContainerStyle`
// automatically reserves space under the last child equal to the
// keyboard height + the toolbar rail height (whenever the keyboard is
// up, the portal is mounted, so the inset includes the toolbar). The
// keyboard toolbar is the V1-style `EditorHelperRails` (toolbar keys +
// VoiceCommandButton) hosted by `KeyboardToolbarTarget`. Results panel
// and rename dialog are intentionally omitted (deferred to V1).
import type { RunResponse } from '../domain';
import { Logger } from '@lib/logging';
import { toast } from 'sonner-native';

import { useUnsavedChangesRequestStore } from '../state/unsavedChangesRequestStore';
import {
  Button,
  IconButton,
  KeyboardToolbarAwareScrollView,
  KeyboardToolbarTarget,
  ScreenHeader,
  Spinner,
  Text,
  View,
} from '@lib/tamagui-design-system';
import type { EditAlternative } from '@lib/voice-edit-dsl';
import { Stack, router } from 'expo-router';
import {
  History,
  Info,
  LogIn,
  LogOut,
  Mic,
  Pencil,
  MoreHorizontal,
  Wand2,
} from '@lib/tamagui-design-system/icons';
import React from 'react';
import { ScrollView, TextInput, type LayoutChangeEvent } from 'react-native';
import { XStack, YStack, useTheme } from '@lib/tamagui-design-system/primitives';

import { useLeetCodePorts } from '../state/ports';
import {
  EditorInput,
  type EditorInputHandle,
} from '../code-editor/core/EditorInput';
import { applyVoiceEdit } from '../code-editor/core/applyVoiceEdit';
import { type Selection } from '../code-editor/core/types';
import { EditorHelperRails } from '../code-editor/helperbars/EditorHelperRails';
import { ProblemCatalog } from '../domain/problemCatalog';
import { compactVoiceError } from '../domain/voiceEditIdentity';
import { useVoiceEdit } from '../state/useVoiceEdit';
import { useLeetCodeAuthStore } from '../state/authStore';
import { useCodeHistoryRequestStore } from '../state/codeHistoryRequestStore';
import { useCodeHistoryStore } from '../state/codeHistoryStore';
import { useResetCodeRequestStore } from '../state/resetCodeRequestStore';

import { AlternativePicker } from './AlternativePicker';
import { DiffToggle } from './DiffToggle';
import {
  LanguagePicker,
  LanguageSheet,
  type EditorLanguage,
} from './LanguagePicker';
import { VoiceRecordingToolbar } from './VoiceRecordingToolbar';
import { VoiceReviewSheet } from './VoiceReviewSheet';

// The editor takes the leftover height; the test input is a fixed block at
// the bottom. `flexGrow: 1` on the scroll view's content container lets the
// editor expand, and the minimums keep both usable once the keyboard shrinks
// the visible area below the editor's flex share.
const EDITOR_MIN_HEIGHT = 280;
const TEST_INPUT_MIN_HEIGHT = 200;

function defaultTestInput(problem: ReturnType<typeof ProblemCatalog.getById>) {
  const testCase = problem?.testCases?.[0];
  if (!testCase) return '';
  return Object.values(testCase.inputs)
    .map((v) => JSON.stringify(v))
    .join('\n');
}

function pickStarterCode(
  starterCode: Record<string, string> | undefined,
  lang: EditorLanguage
): string {
  if (!starterCode) return '';
  return starterCode[lang] ?? Object.values(starterCode)[0] ?? '';
}

const VOICE_ERROR_AUTO_DISMISS_MS = 8_000;

// Which error text a run carries, never the text: it echoes the user's code.
function runErrorKind(result: RunResponse): 'compile' | 'runtime' | 'none' {
  if (result.compile_error) return 'compile';
  return result.runtime_error ? 'runtime' : 'none';
}

export function ProblemEditorScreen({
  id,
  seed,
  onWorkspaceApi,
}: {
  id?: string;
  /** Dev bundles only: the code the editor opens on, so a device-lab job can start mid-problem. */
  seed?: string;
  onWorkspaceApi?: (api: {
    getCode(): string;
    applyEdit(alt: EditAlternative): { modifiedText: string };
    language: string;
  }) => void;
}) {
  const problem = React.useMemo(
    () => (id ? ProblemCatalog.getById(id) : undefined),
    [id]
  );

  const [lang, setLang] = React.useState<EditorLanguage>('python3');
  const [editorText, setEditorText] = React.useState(
    () => seed || pickStarterCode(problem?.starterCode, lang)
  );
  const [editorSelection, setEditorSelection] = React.useState<Selection>({
    start: 0,
    end: 0,
  });

  const [testInput, setTestInput] = React.useState(() =>
    problem ? defaultTestInput(problem) : ''
  );
  const inputRef = React.useRef<React.ElementRef<typeof TextInput>>(null);
  const editorRef = React.useRef<EditorInputHandle>(null);
  const [localRunning, setLocalRunning] = React.useState(false);
  const [runOutcome, setRunOutcome] = React.useState<RunResponse | null>(null);
  // The buffer the current outcome describes. A result is a claim about the
  // code that produced it, so once the code moves the card is describing
  // something that no longer exists -- run to Accepted, edit the solution into
  // nonsense, and a green "12/12" sat over it. Compared by value rather than
  // cleared on every change because the editor echoes programmatic writes
  // (format, language switch, voice apply) back through onChangeText, and an
  // echo carries identical text.
  const outcomeSourceRef = React.useRef<string | null>(null);
  // The gateway is the fetch client for leetcode-backend. It backs three
  // buttons: `Run Local` and `Format` execute on the backend with no
  // leetcode.com/session, while `Run` needs an auth'd session.
  const { gateway } = useLeetCodePorts();
  const scrollRef = React.useRef<ScrollView>(null);
  // Which outcome has already been scrolled to, so a re-layout (keyboard
  // raise, rotation) does not yank the page back.
  const revealedOutcomeRef = React.useRef<unknown>(null);

  // The result lands BELOW the fold, under the keyboard rails -- measured at
  // y438-501 against rails occupying y480-582, which slices off the line
  // carrying the failure reason. The scroll view already reserves room for the
  // rails, so the content is reachable; nothing was bringing it into view at
  // the one moment the user is waiting for it.
  const revealRunOutcome = (event: LayoutChangeEvent) => {
    if (revealedOutcomeRef.current === runOutcome) return;
    revealedOutcomeRef.current = runOutcome;
    const { y } = event.nativeEvent.layout;
    Logger.info('editor_v2.run_outcome.reveal', { y });
    scrollRef.current?.scrollTo({ y: Math.max(0, y - 24), animated: true });
  };
  const [runState, setRunState] = React.useState<
    'idle' | 'testing' | 'submitting'
  >('idle');
  const [diffMode, setDiffMode] = React.useState(false);
  const [diffRange, setDiffRange] = React.useState<{
    start: number;
    end: number;
  } | null>(null);
  const theme = useTheme();
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
    (reason: Parameters<typeof recordSnapshot>[0]['reason']) => {
      if (!problem) return;
      recordSnapshot({
        problemId: problem.id,
        language: lang,
        code: editorText,
        reason,
      });
    },
    [problem, lang, editorText, recordSnapshot]
  );

  const busy = runState !== 'idle';
  const canRun = isLoggedIn && !busy;
  const voiceEdit = useVoiceEdit(editorText, lang, `${id ?? 'problem'}.m4a`, {
    problemId: problem?.id ?? null,
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

  const handleLanguageChange = React.useCallback(
    (nextLang: EditorLanguage) => {
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
      setRunOutcome(null);
    },
    [lang, problem?.id, problem?.starterCode, snapshot]
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

  // Run Local: execute the code against the problem's bundled testCases on
  // leetcode-backend (no leetcode.com, no auth). Only `Submit` reaches
  // leetcode.com.
  const handleEditorChange = React.useCallback((next: string) => {
    setEditorText(next);
    const source = outcomeSourceRef.current;
    if (source !== null && next !== source) {
      outcomeSourceRef.current = null;
      setRunOutcome(null);
    }
  }, []);

  const handleRunLocally = React.useCallback(async () => {
    setLocalRunning(true);
    setRunOutcome(null);
    const startedAt = Date.now();
    try {
      const result = await gateway.runLocal({
        lang,
        code: editorText,
        methodName: problem?.methodName ?? 'twoSum',
        testCases: problem?.testCases ?? [],
      });
      setRunOutcome(result);
      outcomeSourceRef.current = editorText;
      Logger.info('editor_v2.run_local.result', {
        state: result.state,
        totalCorrect: result.total_correct,
        totalTestcases: result.total_testcases,
        problemId: problem?.id,
        lang,
        statusMsg: result.status_msg,
        errorKind: runErrorKind(result),
        ms: Date.now() - startedAt,
      });
      // A run that reaches the backend and fails its tests is not an error --
      // it is the normal case while solving. Only the transport failing is.
      const passed =
        result.state === 'SUCCESS' &&
        result.total_correct === result.total_testcases;
      const tally = `${result.total_correct}/${result.total_testcases} tests`;
      if (passed) toast.success('All tests passed', { description: tally });
      else
        toast.error('Tests failed', {
          description: `${tally} · ${result.status_msg}`,
        });
    } catch (err) {
      setRunOutcome({
        source: 'backend-run',
        kind: 'test',
        state: 'FAILURE',
        requestId: '',
        run_success: false,
        status_msg: 'Error',
        total_correct: 0,
        total_testcases: 0,
        runtime_error: err instanceof Error ? err.message : String(err),
      });
      Logger.withError(err)
        .withMetadata({
          problemId: problem?.id,
          lang,
          ms: Date.now() - startedAt,
        })
        .error('editor_v2.run_local.error');
      toast.error('Run failed', {
        description:
          err instanceof Error ? err.message : 'The backend did not respond.',
      });
    } finally {
      setLocalRunning(false);
    }
  }, [editorText, gateway, problem, lang]);

  // Run: submit the current code + the custom testInput to the auth'd
  // LeetCode bridge and await the judge result. Mirrors V1's `runTest`
  // (V1:308). Requires `canRun` to be true (logged in + no other run in
  // flight).
  const runTest = React.useCallback(async () => {
    if (!canRun || !problem) return;
    setRunState('testing');
    const startedAt = Date.now();
    try {
      const testResult = await gateway.test({
        slug: problem.id,
        lang,
        questionId: problem.leetcodeId,
        code: editorText,
        dataInput: testInput,
      });
      // The live gateway's test returns { interpret_id }; the port types the
      // whole judge surface as RunResponse, so the handle rides a narrow cast.
      const interpretId = (testResult as unknown as { interpret_id?: string })
        .interpret_id;
      if (!interpretId) {
        Logger.warn('editor_v2.run.no_interpret_id', {
          problemId: problem.id,
          lang,
        });
        return;
      }
      const result = await gateway.poll(interpretId);
      Logger.info('editor_v2.run.result', {
        state: result.state,
        statusMsg: result.status_msg,
        totalCorrect: result.total_correct,
        totalTestcases: result.total_testcases,
        problemId: problem.id,
        lang,
        ms: Date.now() - startedAt,
      });
    } catch (err) {
      Logger.withError(err)
        .withMetadata({ problemId: problem.id, ms: Date.now() - startedAt })
        .error('editor_v2.run.error');
    } finally {
      setRunState('idle');
    }
  }, [canRun, gateway, problem, editorText, testInput, lang]);

  const handleVoiceApply = React.useCallback(
    (alt: EditAlternative) => {
      // The commands themselves, because "the edit came out wrong" has been
      // undiagnosable from this log more than once: the planner is
      // nondeterministic, so the offending command cannot be reproduced after
      // the fact by asking it again. Offsets are the part that matters; text
      // is the user's code, so it is logged as a length.
      Logger.info('editor_v2.voice_apply', {
        alternativeId: alt.id,
        confidence: alt.confidence,
        problemId: problem?.id,
        codeLength: editorText.length,
        commands: alt.commands.map((c) =>
          Object.fromEntries(
            Object.entries(c).map(([k, v]) =>
              typeof v === 'string' && k !== 'type'
                ? [`${k}Length`, v.length]
                : [k, v]
            )
          )
        ),
      });
      const result = applyVoiceEdit(editorText, alt);
      snapshot('voice-edit');
      setDiffRange(result.diffRange);
      setDiffMode(true);
      setEditorText(result.modifiedText);
      editorRef.current?.setTextAndSelection(
        result.modifiedText,
        result.selection
      );
      editorRef.current?.showDiff(result.diffRange.start, result.diffRange.end);
      const pickedIndex =
        voiceEdit.state.phase === 'choosing'
          ? voiceEdit.state.alternatives.findIndex((a) => a.id === alt.id)
          : -1;
      voiceEdit.markApplied({
        ...(pickedIndex >= 0 ? { pickedIndex } : {}),
        codeAfter: result.modifiedText,
      });
      voiceEdit.dismiss();
    },
    [editorText, voiceEdit, problem?.id, snapshot]
  );

  React.useEffect(() => {
    const alt = voiceEdit.consumeAutoApply();
    if (alt) {
      handleVoiceApply(alt);
    }
  }, [voiceEdit, handleVoiceApply]);

  // The plan tab reads the buffer and applies edits through the same path as
  // a voice apply (minus the voice bookkeeping); re-register when they move.
  React.useEffect(() => {
    if (!onWorkspaceApi) return;
    onWorkspaceApi({
      getCode: () => editorText,
      applyEdit: (alt: EditAlternative) => {
        const result = applyVoiceEdit(editorText, alt);
        snapshot('voice-edit');
        setDiffRange(result.diffRange);
        setDiffMode(true);
        setEditorText(result.modifiedText);
        editorRef.current?.setTextAndSelection(
          result.modifiedText,
          result.selection
        );
        editorRef.current?.showDiff(
          result.diffRange.start,
          result.diffRange.end
        );
        return { modifiedText: result.modifiedText };
      },
      language: lang,
    });
  }, [editorText, lang, onWorkspaceApi, snapshot]);

  const handleDiffToggle = React.useCallback(
    (enabled: boolean) => {
      Logger.info('editor_v2.diff_toggle', { enabled, problemId: problem?.id });
      setDiffMode(enabled);
      if (enabled && diffRange) {
        editorRef.current?.showDiff(diffRange.start, diffRange.end);
      } else {
        editorRef.current?.hideDiff();
      }
    },
    [diffRange, problem?.id]
  );

  const handleVoiceStart = React.useCallback(async () => {
    Logger.info('editor_v2.voice_start', { problemId: problem?.id });
    await voiceEdit.start();
  }, [voiceEdit, problem?.id]);

  const handleTextEditStart = React.useCallback(() => {
    Logger.info('editor_v2.text_edit_start', { problemId: problem?.id });
    voiceEdit.startTextEdit();
  }, [voiceEdit, problem?.id]);

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

  const handleRenameSymbol = React.useCallback(
    (symbol: string) => {
      Logger.info('editor_v2.rename_symbol_request', {
        symbolLength: symbol.length,
        problemId: problem?.id,
      });
    },
    [problem?.id]
  );

  const accepted = runOutcome?.status_msg === 'Accepted';

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
        setRunOutcome(null);
        // The diff range belongs to the code that was just discarded.
        setDiffRange(null);
        setDiffMode(false);
        editorRef.current?.hideDiff();
      },
      onCancel: () => {
        Logger.info('editor_v2.reset_code.cancel', { problemId: problem.id });
      },
    });
    router.push(`/leetcode/${problem.id}/reset-code`);
  }, [problem, lang, snapshot]);

  return (
    <View flex={1} bg="$background" testID="ProblemEditorScreen">
      <Stack.Screen
        options={{
          // A custom header, not headerTitle/headerRight: the stock bar has one
          // text slot, so the category and difficulty had nowhere to go and the
          // back button carried the parent's title as a long label beside them.
          header: () => (
            <ScreenHeader
              testID="ProblemEditorHeader"
              title={problem?.title ?? 'Problem'}
              subtitle={problemSubtitle}
              backAccessibilityLabel="Back to the problem list"
              onBack={() => {
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
              }}
              actions={[
                {
                  icon: Info,
                  accessibilityLabel: 'Problem description',
                  testID: 'ProblemEditorInfoButton',
                  onPress: () => {
                    Logger.info('editor_v2.info_tap', {
                      problemId: problem?.id,
                    });
                    editorRef.current?.blur();
                    inputRef.current?.blur();
                    if (problem)
                      router.push(`/leetcode/${problem.id}/description`);
                  },
                },
              ]}
              menu={{
                icon: MoreHorizontal,
                accessibilityLabel: 'More actions',
                testID: 'ProblemEditorMenuButton',
                items: [
                  {
                    icon: Wand2,
                    label: 'Format',
                    onPress: () => {
                      Logger.info('editor_v2.format.tap', {
                        problemId: problem?.id,
                        from: 'header_menu',
                      });
                      void handleFormat();
                    },
                    testID: 'ProblemEditorMenuFormat',
                  },
                  {
                    icon: History,
                    label: 'Version history',
                    detail:
                      historyCount > 0
                        ? `${historyCount} saved`
                        : 'No versions yet',
                    onPress: () => {
                      Logger.info('editor_v2.history_tap', {
                        problemId: problem?.id,
                        count: historyCount,
                      });
                      editorRef.current?.blur();
                      inputRef.current?.blur();
                      if (!problem) return;
                      // Register the restore target BEFORE pushing: the sheet
                      // is its own route, so it shares no React tree with this
                      // screen and cannot reach setEditorText directly.
                      useCodeHistoryRequestStore.getState().request({
                        problemId: problem.id,
                        onRestore: (code, language) => {
                          snapshot('reset');
                          setEditorText(code);
                          editorRef.current?.setTextAndSelection(code, {
                            start: code.length,
                            end: code.length,
                          });
                          if (language !== lang) {
                            setLang(language as EditorLanguage);
                          }
                        },
                      });
                      router.push(`/leetcode/${problem.id}/history`);
                    },
                    testID: 'ProblemEditorMenuHistory',
                  },
                  {
                    icon: isLoggedIn ? LogOut : LogIn,
                    label: isLoggedIn ? 'Log out' : 'Login',
                    detail: isLoggedIn ? (username ?? undefined) : 'LeetCode',
                    onPress: () => {
                      if (isLoggedIn) {
                        Logger.info('editor_v2.logout_tap', {
                          problemId: problem?.id,
                        });
                        clearCredentials();
                        return;
                      }
                      Logger.info('editor_v2.login_tap', {
                        problemId: problem?.id,
                      });
                      router.push('/leetcode/login');
                    },
                    testID: 'ProblemEditorMenuLogin',
                  },
                ],
              }}
            />
          ),
        }}
      />
      <KeyboardToolbarTarget
        testID="ProblemEditorToolbarTarget"
        toolbarTestID="EditorHelperRails"
        toolbar={() => (
          <EditorHelperRails
            editorRef={editorRef}
            source={editorText}
            selection={editorSelection}
            onConfigure={() => {
              Logger.info('editor_v2.toolbar_configure', {
                problemId: problem?.id,
              });
              router.push('/leetcode/toolbar');
            }}
            onRenameSymbol={handleRenameSymbol}
            onDismissKeyboard={() => {
              Logger.debug('editor_v2.keyboard_dismiss', {
                problemId: problem?.id,
              });
              editorRef.current?.blur();
              inputRef.current?.blur();
            }}
          />
        )}
      >
        <KeyboardToolbarAwareScrollView
          ref={scrollRef}
          testID="ProblemEditorScroll"
          keyboardShouldPersistTaps="handled"
        >
          {/* Editor (CodeMirror WebView) — fixed min-h so the ScrollView has
              something concrete to scroll past when the keyboard shrinks the
              visible area. */}
          <YStack
            flex={1}
            minH={EDITOR_MIN_HEIGHT}
            px="$3"
            py="$2"
            testID="ProblemEditorEditorZone"
          >
            <EditorInput
              ref={editorRef}
              testID="ProblemEditorInput"
              value={editorText}
              onChangeText={handleEditorChange}
              onSelectionChange={setEditorSelection}
            />
          </YStack>

          <XStack
            items="center"
            justify="space-between"
            px="$3"
            pt="$2"
            testID="ProblemEditorDiffRow"
          >
            <LanguagePicker
              value={lang}
              onOpen={() => setLanguageSheetOpen(true)}
              testID="ProblemEditorLanguagePicker"
            />
            {diffRange ? (
              <DiffToggle
                enabled={diffMode}
                onToggle={handleDiffToggle}
                testID="ProblemEditorDiffToggle"
              />
            ) : null}
          </XStack>

          {/* Action row — Format / Run Local / Run, mirrors V1's action row
              minus the Submit button which the user explicitly dropped. */}
          <XStack
            items="center"
            gap="$2.5"
            px="$3"
            py="$1.5"
            testID="ProblemEditorActions"
          >
            <Button
              testID="ProblemEditorFormatButton"
              variant="outline"
              size="$2"
              pressLog="off"
              onPress={() => {
                Logger.info('editor_v2.format.tap', { problemId: problem?.id });
                void handleFormat();
              }}
            >
              Format
            </Button>
            <Button
              testID="ProblemEditorRunLocalButton"
              variant="outline"
              size="$2"
              disabled={localRunning}
              pressLog="off"
              onPress={() => {
                Logger.info('editor_v2.run_local.tap', {
                  problemId: problem?.id,
                });
                void handleRunLocally();
              }}
            >
              {localRunning ? 'Running…' : 'Run Local'}
            </Button>
            <Button
              testID="ProblemEditorRunButton"
              variant="outline"
              size="$2"
              disabled={!canRun}
              pressLog="off"
              onPress={() => {
                Logger.info('editor_v2.run.tap', { problemId: problem?.id });
                void runTest();
              }}
            >
              {runState === 'testing' ? 'Running…' : 'Run'}
            </Button>
            <Button
              testID="ProblemEditorResetButton"
              variant="outline"
              size="$2"
              pressLog="off"
              onPress={handleResetCode}
            >
              Reset
            </Button>
            <YStack flex={1} />
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
                editorRef.current?.blur();
                inputRef.current?.blur();
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
                editorRef.current?.blur();
                inputRef.current?.blur();
                void handleVoiceStart();
              }}
            />
          </XStack>

          {/* Run Local result — server-executed pass/fail summary. */}
          {runOutcome ? (
            <YStack
              mx="$3"
              mb="$1"
              px="$3"
              py="$2"
              rounded="$3"
              borderWidth={1}
              borderColor={accepted ? '$green9' : '$red9'}
              onLayout={revealRunOutcome}
              testID="ProblemEditorRunOutcome"
            >
              <Text
                variant="strong"
                color={accepted ? '$green10' : '$red10'}
                testID="ProblemEditorRunOutcomeStatus"
              >
                {runOutcome.status_msg}
              </Text>
              {runOutcome.total_testcases > 0 ? (
                <Text variant="muted" testID="ProblemEditorRunOutcomeTotals">
                  {`${runOutcome.total_correct} / ${runOutcome.total_testcases} test cases passed`}
                </Text>
              ) : null}
              {runOutcome.runtime_error ? (
                <Text
                  variant="code"
                  color="$red10"
                  testID="ProblemEditorRunOutcomeRuntimeError"
                >
                  {runOutcome.runtime_error}
                </Text>
              ) : null}
              {runOutcome.compile_error ? (
                <Text
                  variant="code"
                  color="$red10"
                  testID="ProblemEditorRunOutcomeCompileError"
                >
                  {runOutcome.compile_error}
                </Text>
              ) : null}
            </YStack>
          ) : null}

          {/* TestInput — fixed min-h, also scrollable. */}
          <YStack
            minH={TEST_INPUT_MIN_HEIGHT}
            px="$3"
            py="$2"
            testID="ProblemEditorTestInputZone"
          >
            <TextInput
              ref={inputRef}
              testID="ProblemEditorTestInput"
              value={testInput}
              onChangeText={setTestInput}
              multiline
              placeholder="Custom test input"
              // A bare RN TextInput defaults to black text, which vanished on the dark theme.
              placeholderTextColor={theme.color10?.val}
              style={{
                color: theme.color?.val,
                flex: 1,
                borderRadius: 8,
                paddingHorizontal: 10,
                paddingVertical: 6,
                fontFamily: 'Menlo, Courier, monospace',
                fontSize: 13,
                textAlignVertical: 'top',
              }}
            />
          </YStack>
        </KeyboardToolbarAwareScrollView>
      </KeyboardToolbarTarget>
      <LanguageSheet
        open={languageSheetOpen}
        onOpenChange={setLanguageSheetOpen}
        value={lang}
        onChange={handleLanguageChange}
        testID="ProblemEditorLanguagePicker"
      />
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
