import { normalizeText, type CompletionEntry, type Language, type Reply } from '@lib/code-editor-api';
import { Logger } from '@lib/logging';
import { View } from '@lib/tamagui-design-system';
import { decode } from '@lib/webview-rpc';
import React, {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import { Platform } from 'react-native';
import { WebView, type WebViewMessageEvent } from 'react-native-webview';

import type {
  CodeEditorPort,
  EditorConfig,
  FromEditorMessage,
  ToEditorMessage,
} from '../../domain/ports';
import type { BlockEditor } from '../../domain/blocks/blockEditor';
import { createCaretDrag, moveCaretBy, type CaretDrag } from '../../domain/editor/caretDrag';
import type { Delta } from '../../domain/editor/cursorDrag';
import type { Motion } from '../../domain/editor/cursorMotion';
import { writeThrough, type WriteRequest } from '../../domain/editor/documentWrite';
import { createColdStart, nextEditorNth } from '../../domain/editor/coldStart';
import { clearEditMark, markEdit, setEditMarkHidden } from '../../domain/editor/editMark';
import { createForwardingPost } from '../../domain/editor/editorForwarding';
import { applyPlannedEdit, readBasis, type EditBasis, type EditOutcome, type EditTarget, type PlannedEdit } from '../../domain/editor/voiceEditPlan';
import { lineHeightFor } from '../../domain/editor-settings/editorPreferences';
import { useEditorPreferencesStore } from '../../state/editor/editorPreferencesStore';
import { useLeetCodePorts } from '../../state/ports';
import { useEditorAppearance } from './useEditorAppearance';
import type { EditResult } from '../../domain/editor/editing';
import { checkpointWithin, clearErrorLine, showErrorLine, type ErrorCheckpoint, type ErrorJump, type ErrorPlace } from '../../domain/editor/errorJump';
import type { Selection } from '../../domain/editor/types';
import { createEditorTextSource, EditorTextMirror } from './EditorTextMirror';

// App default 400ms flush — same cadence as every other Logger.

// A cold phone WebView takes seconds to evaluate the CodeMirror bundle; past
// this without 'hydrated' the editor is dead or blank.
const HYDRATE_TIMEOUT_MS = 15_000;

// A run waits this long for the editor's text; a page that is slower than this runs on the screen's text, with no error line.
const CHECKPOINT_DEADLINE_MS = 500;

export type CaretDragSession = {
  move: (delta: Delta) => void;
  end: () => Promise<void>;
};

const NO_CARET_DRAG: CaretDragSession = { move: () => undefined, end: () => Promise.resolve() };

export type EditorInputHandle = EditTarget & {
  focus: () => void;
  blur: () => void;
  applySelection: (sel: Selection) => void;
  insertAtCursor: (text: string, cursorOffset?: number) => void;
  /** Text as if typed at the caret: auto-closed brackets, re-indented `else:` and the rest of what typing does. */
  typeText: (text: string) => void;
  /** Replace the whole document and place the selection: one hunk, one undo step. */
  setTextAndSelection: (text: string, sel: Selection) => void;
  /** Apply a pure whole-document edit to the editor's own text and selection (the toolbar's Tab on a selection, Dedent, Comment, Newline). */
  transformDocument: (transform: (text: string, sel: Selection) => EditResult) => void;
  /** Starts a caret drag at the finger's grab: the editor is focused once, the page's selection events are held while it lasts, and React is told the selection once, at `end`. */
  beginCaretDrag: () => CaretDragSession;
  /** One caret motion from the editor's own caret, as an accessibility action asks for it. */
  moveCaret: (motion: Motion) => Promise<void>;
  /** Whether the editor shows its suggestion list now, as far as the last event said. */
  isCompletionOpen: () => boolean;
  /** Takes the suggestion the list has selected; false when nothing was open or taken. */
  acceptCompletion: () => Promise<boolean>;
  /** The editor's text, revision and instance in one read, after the writes still queued; null when the editor is not ready or does not answer in time. */
  checkpoint: () => Promise<ErrorCheckpoint | null>;
  /** Marks the reported line of the checkpoint's text and scrolls it into view, with no selection or focus change; never throws. */
  showErrorLine: (checkpoint: ErrorCheckpoint, place: ErrorPlace | undefined) => Promise<ErrorJump>;
  clearErrorLine: () => Promise<void>;
  /** The typed editor API for a client that reads and edits through it (the block view); null until the editor is ready and initialised. `settled` resolves when the writes still queued have landed. */
  api: () => BlockEditor | null;
  undo: () => void;
  redo: () => void;
  /** Shows or hides the highlight of what the last voice or plan edit changed; the mark stays and follows the text either way. */
  setEditMarkHidden: (hidden: boolean) => void;
  clearEditMark: () => void;
};

type Props = {
  value: string;
  onChangeText: (text: string) => void;
  onSelectionChange: (sel: Selection) => void;
  onHydratedChange?: (hydrated: boolean) => void;
  onFocusChange?: (focused: boolean) => void;
  onUndoAvailabilityChange?: (canUndo: boolean) => void;
  documentKey?: string;
  /** The language the editor highlights and indents in; changing it switches the live editor. */
  language?: Language;
  /** Suggestions while typing: off when absent or not enabled. The names are what the problem's own signature adds. */
  completion?: { enabled: boolean; entries: readonly CompletionEntry[] };
  testID?: string;
  appearance?: 'panel' | 'canvas';
};

/**
 * CodeMirror 6 editor mounted inside a WebView.
 *
 * The HTML template is static; all configuration and edits flow over a
 * bidirectional postMessage protocol (the message types are in domain/ports.ts). Adapted from the
 * mobile-learning-apps reference: DI/observability, NativeWind, the design-system
 * theme store, and Maestro test guards have been stripped out for this app.
 */
export const EditorInput = forwardRef<EditorInputHandle, Props>(
  function EditorInput(
    {
      value,
      onChangeText,
      onSelectionChange,
      onHydratedChange,
      onFocusChange,
      onUndoAvailabilityChange,
      documentKey,
      language = 'python',
      completion,
      testID,
      appearance = 'panel',
    },
    ref
  ) {
    const webViewRef = useRef<WebView>(null);
    const isReadyRef = useRef(false);
    const hydratedRef = useRef(false);
    const queuedFocusRef = useRef(false);
    const queuedSelectionRef = useRef<Selection | null>(null);
    /**
     * A programmatic text write that arrived before the WebView could take
     * it. `sendMessage` drops messages while the view is not ready, and
     * `setTextAndSelection` used to record the text as delivered anyway --
     * so after a remount (leaving the reset-code route, a Metro reload) a
     * voice edit updated React state, the view kept the starter code, the
     * value-sync effect believed the view already had the edit, and Run
     * Local compiled code the editor never showed. Seen live: the editor
     * displayed the 68-character stub while the backend reported a
     * CompileError on a `while` at line 5.
     */
    const queuedTextRef = useRef<{ text: string; sel: Selection } | null>(null);
    const valueRef = useRef(value);
    valueRef.current = value;
    const onSelectionChangeRef = useRef(onSelectionChange);
    onSelectionChangeRef.current = onSelectionChange;
    // the live caret drag, and the last selection the page reported while it ran: React hears the drag's end once, not every call
    const caretDragRef = useRef<{ drag: CaretDrag; held: Selection | null } | null>(null);
    const initialValueRef = useRef(value);
    const appliedDocumentKey = useRef(documentKey);
    // Tracks the last text CM6 reported via contentChanged, so we can detect
    // external store changes (e.g. replaceAll) and push them back to CM6.
    const lastWebViewTextRef = useRef(value);
    const [textMirror] = useState(() => createEditorTextSource(value));
    const holdText = useCallback(
      (text: string) => {
        lastWebViewTextRef.current = text;
        textMirror.set(text);
      },
      [textMirror]
    );
    // Tracks whether we've sent the 'init' message to the editor.
    const isInitializedRef = useRef(false);
    // the language the page was last told: `init` carries it, a later change is posted as an update
    const languageRef = useRef(language);
    languageRef.current = language;
    const appliedLanguageRef = useRef(language);
    // the suggestion config the page was last given, and whether its list is open: the Indent key takes a suggestion only while one is shown
    const completionRef = useRef(completion);
    completionRef.current = completion;
    const sentCompletionRef = useRef<string | null>(null);
    const completionOpenRef = useRef(false);
    const initLanguageRef = useRef(language);

    const fontSize = useEditorPreferencesStore((s) => s.fontSize);
    const wordWrap = useEditorPreferencesStore((s) => s.wrap);
    const tabSize = useEditorPreferencesStore((s) => s.tabSize);
    const showWhitespace = useEditorPreferencesStore((s) => s.whitespace);
    const canvas = appearance === 'canvas';
    const { backgroundColor, syntaxPalette, isDark, palette, paletteId } = useEditorAppearance(canvas);
    const lineHeight = lineHeightFor(fontSize);
    // Editor chrome colors (gutter + border) are intentionally tied to the
    // editor palette's appearance, not the OS theme — they live inside the
    // embedded WebView, so the surrounding app's tokens don't apply.
    const gutterColor = isDark ? '#5c6370' : '#9aa0a6';
    const borderColor = canvas ? backgroundColor : isDark ? '#3e4451' : '#dadce0';

    // HTML is static — no config injected. All config flows via postMessage.
    const { editorBridge, clock } = useLeetCodePorts();
    const html = useMemo(() => editorBridge.html(), [editorBridge]);
    const coldStartRef = useRef<ReturnType<typeof createColdStart> | null>(null);
    if (coldStartRef.current === null) {
      coldStartRef.current = createColdStart({ clock, nth: nextEditorNth(), htmlBytes: html.length });
      coldStartRef.current.mark('mount');
    }
    const pageEvalMsRef = useRef<number | undefined>(undefined);

    // One CodeEditorPort per editor instance, bound to this WebView: post()
    // drives the injectJavaScript channel, onMessage() fans webview events out
    // to subscribers (the webview stub in flow tests routes through the same
    // shape).
    const messageListenersRef = useRef<Set<(m: FromEditorMessage) => void>>(
      new Set()
    );
    // The typed editor API over the same WebView: rpc frames come back through onMessage, requests go down as injected scripts.
    const [apiBridge] = useState(() => editorBridge.connect(() => webViewRef.current));
    const editorPort = useMemo<CodeEditorPort>(() => {
      const legacyPost = (msg: ToEditorMessage) => {
        const webView = webViewRef.current;
        if (!webView) return;
        webView.injectJavaScript(
          `window.dispatchEvent(new MessageEvent('message', { data: ${JSON.stringify(JSON.stringify(msg))} })); true;`
        );
      };
      return {
        post: createForwardingPost(apiBridge.client, legacyPost, {
          blurThroughApi: Platform.OS !== 'ios',
          isDisposing: () => !isReadyRef.current,
        }),
        onMessage(listener) {
          messageListenersRef.current.add(listener);
          return () => {
            messageListenersRef.current.delete(listener);
          };
        },
      };
    }, [apiBridge]);

    const sendMessage = useCallback(
      (msg: ToEditorMessage) => {
        if (!isReadyRef.current) return;
        if (
          msg.type === 'focus' ||
          msg.type === 'blur' ||
          msg.type === 'init' ||
          msg.type === 'setSelection'
        ) {
          Logger.debug('editor.send_message', {
            type: msg.type,
            ...(msg.type === 'setSelection'
              ? { start: msg.start, end: msg.end }
              : {}),
          });
        }
        editorPort.post(msg);
      },
      [editorPort]
    );

    // The one writer of the document from outside the editor (the value sync, the reconcile at hydration, voice and plan edits, the
    // whole-document keys of the toolbar): the hunk that differs, applied at the revision it was planned against, so it is one undo
    // step and a keystroke that landed meanwhile makes the call stale (it is planned again) instead of being overwritten. `next` is
    // given the editor's own text and selection when the write runs, so a queued write uses the latest value and a key works on
    // what is really there. The mirror (`lastWebViewTextRef`) moves only when the write has landed or needed nothing, or when the
    // editor says what it holds: a mirror set ahead of the editor once ran code the editor never showed.
    const writeChainRef = useRef<Promise<void>>(Promise.resolve());
    const writeDocument = useCallback(
      (request: WriteRequest & {
        /** the old script focused the editor with the selection that came with a text: voice and plan edits keep that */
        focus?: boolean;
      }): Promise<void> => {
        const run = async () => {
          const outcome = await writeThrough(apiBridge.client, request, () => isReadyRef.current);
          if (!outcome) return;
          if (outcome.landed) {
            holdText(outcome.text);
            if (request.focus) void apiBridge.client.focus();
            return;
          }
          // The write did not land: React is brought back to what the editor holds now, and if the editor cannot be read the mirror
          // keeps what the editor last reported, so the value sync tries again the next time it runs.
          Logger.warn('editor.write.failed', { label: request.label, code: outcome.code, healed: outcome.editorText !== null });
          if (outcome.editorText !== null) {
            holdText(outcome.editorText);
            onChangeText(outcome.editorText);
          }
        };
        writeChainRef.current = writeChainRef.current.then(run, run);
        return writeChainRef.current;
      },
      [apiBridge, onChangeText, holdText]
    );

    // The edit mark's calls wait behind a write that is still queued, so they act on the text that write is about to put there.
    const queueMark = useCallback(
      (effect: string, job: () => Promise<Reply<unknown> | null>): void => {
        const run = async () => {
          if (!isReadyRef.current || !isInitializedRef.current) return;
          const reply = await job();
          if (reply && !reply.ok) Logger.info('editor.edit_mark.failed', { effect, code: reply.error.code });
        };
        writeChainRef.current = writeChainRef.current.then(run, run);
      },
      []
    );

    const updateHydrated = useCallback(
      (next: boolean) => {
        if (hydratedRef.current === next) {
          return;
        }
        Logger.info('editor.update_hydrated', { next });
        hydratedRef.current = next;
        onHydratedChange?.(next);
      },
      [onHydratedChange]
    );

    useImperativeHandle(
      ref,
      () => ({
        focus() {
          Logger.info('editor.focus', { effect: 'focus' });
          if (!isReadyRef.current || !isInitializedRef.current) {
            queuedFocusRef.current = true;
            return;
          }
          sendMessage({ type: 'focus' });
        },
        blur() {
          Logger.debug('editor.blur', {
            isReady: isReadyRef.current,
            isInitialized: isInitializedRef.current,
            hasWebView: webViewRef.current != null,
          });
          sendMessage({ type: 'blur' });
          // On web the iframe stays focused after the internal contenteditable
          // blur, so we must blur the iframe itself so document.activeElement
          // is cleared and the keyboard toolbar hides.
          if (Platform.OS === 'web') {
            (webViewRef.current as unknown as { blur?: () => void })?.blur?.();
          }
        },
        applySelection(sel: Selection) {
          Logger.info('editor.apply_selection', { effect: 'applySelection' });
          if (!isReadyRef.current || !isInitializedRef.current) {
            queuedSelectionRef.current = sel;
            return;
          }
          sendMessage({ type: 'setSelection', start: sel.start, end: sel.end });
        },
        insertAtCursor(text: string, cursorOffset?: number) {
          Logger.info('editor.insert_at_cursor', { effect: 'insertAtCursor' });
          sendMessage({ type: 'insertAt', text, offset: cursorOffset });
        },
        typeText(text: string) {
          Logger.info('editor.type_text', { effect: 'typeText' });
          sendMessage({ type: 'typeText', text });
        },
        setTextAndSelection(text: string, sel: Selection) {
          Logger.info('editor.set_text_and_selection', {
            effect: 'setTextAndSelection',
            queued: !isReadyRef.current || !isInitializedRef.current,
          });
          if (!isReadyRef.current || !isInitializedRef.current) {
            // Deliver on `hydrated`, like focus and selection already do. The
            // mirror is left alone: it must describe what the view HAS, not
            // what it was asked for.
            queuedTextRef.current = { text, sel };
            return;
          }
          void writeDocument({ next: () => ({ text, selection: sel }), label: 'react.text', focus: true });
        },
        transformDocument(transform: (text: string, sel: Selection) => EditResult) {
          Logger.info('editor.transform_document', { effect: 'transformDocument' });
          if (!isReadyRef.current || !isInitializedRef.current) return;
          // the editor's own text and caret, not React's, which can be a keystroke behind
          void writeDocument({
            next: (current) => transform(current.text, current.selection),
            label: 'toolbar.transform',
          });
        },
        beginCaretDrag(): CaretDragSession {
          if (!isReadyRef.current || !isInitializedRef.current) return NO_CARET_DRAG;
          caretDragRef.current?.drag.cancel();
          const session = { drag: createCaretDrag(apiBridge.client), held: null as Selection | null };
          caretDragRef.current = session;
          return {
            move: (delta) => session.drag.move(delta),
            async end() {
              const landed = await session.drag.end();
              if (caretDragRef.current !== session) return;
              caretDragRef.current = null;
              const selection = landed ?? session.held;
              if (selection) onSelectionChangeRef.current(selection);
            },
          };
        },
        async moveCaret(motion: Motion) {
          if (!isReadyRef.current || !isInitializedRef.current) return;
          await moveCaretBy(apiBridge.client, motion);
        },
        isCompletionOpen() {
          return completionOpenRef.current;
        },
        async acceptCompletion() {
          const reply = await apiBridge.client.transact([{ op: 'complete' }]);
          return reply.ok && reply.value.changes.length > 0;
        },
        async checkpoint() {
          if (!isReadyRef.current || !isInitializedRef.current) {
            Logger.info('editor_v2.run_error.skipped', { reason: 'not-ready' });
            return null;
          }
          const taken = await checkpointWithin(apiBridge.client, CHECKPOINT_DEADLINE_MS, writeChainRef.current.catch(() => undefined));
          if (!taken) Logger.info('editor_v2.run_error.skipped', { reason: 'no-checkpoint' });
          return taken;
        },
        async showErrorLine(checkpoint: ErrorCheckpoint, place: ErrorPlace | undefined): Promise<ErrorJump> {
          if (!isReadyRef.current || !isInitializedRef.current) return { kind: 'refused', code: 'not-ready' };
          const jump = await showErrorLine(apiBridge.client, checkpoint, place);
          switch (jump.kind) {
            case 'shown':
              Logger.info('editor_v2.run_error.shown', { line: jump.line, from: jump.range.from, to: jump.range.to, column: place?.column });
              break;
            case 'gone':
              Logger.info('editor_v2.run_error.gone', { line: jump.line });
              break;
            case 'refused':
              Logger.info('editor_v2.run_error.refused', { code: jump.code });
              break;
            case 'outside':
              Logger.info('editor_v2.run_error.skipped', { reason: 'outside', line: jump.line });
              break;
            case 'no-location':
              Logger.info('editor_v2.run_error.skipped', { reason: 'no-location' });
              break;
          }
          return jump;
        },
        async clearErrorLine() {
          if (!isReadyRef.current || !isInitializedRef.current) return;
          const refused = await clearErrorLine(apiBridge.client);
          if (refused) Logger.info('editor_v2.run_error.refused', { code: refused, call: 'clear' });
        },
        api() {
          if (!isReadyRef.current || !isInitializedRef.current) return null;
          return {
            client: apiBridge.client,
            settled: async () => {
              await writeChainRef.current.catch(() => undefined);
            },
          };
        },
        undo() {
          Logger.info('editor.undo', { effect: 'undo' });
          sendMessage({ type: 'undo' });
        },
        redo() {
          Logger.info('editor.redo', { effect: 'redo' });
          sendMessage({ type: 'redo' });
        },
        async basis(): Promise<EditBasis | null> {
          if (!isReadyRef.current || !isInitializedRef.current) return null;
          // read after a write that is still queued, so the basis the plan is made for already holds it
          await writeChainRef.current.catch(() => undefined);
          return readBasis(apiBridge.client);
        },
        applyEdit(edit: PlannedEdit): Promise<EditOutcome> {
          const run = async (): Promise<EditOutcome> => {
            if (!isReadyRef.current || !isInitializedRef.current) return { ok: false, reason: 'not-ready', code: 'not-ready' };
            const outcome = await applyPlannedEdit(apiBridge.client, edit);
            // a refused edit leaves the editor and the mirror as they were: a keystroke that made it stale reaches React as any keystroke does
            if (outcome.ok) {
              lastWebViewTextRef.current = outcome.modifiedText;
              if (outcome.landed) {
                const marked = await markEdit(apiBridge.client, outcome.landed, outcome.revision, edit.basis.instance);
                if (!marked.ok) Logger.info('editor.edit_mark.failed', { effect: 'markEdit', code: marked.error.code });
              }
              void apiBridge.client.focus();
            }
            return outcome;
          };
          const outcome = writeChainRef.current.then(run, run);
          writeChainRef.current = outcome.then(() => undefined, () => undefined);
          return outcome;
        },
        setEditMarkHidden(hidden: boolean) {
          Logger.info('editor.edit_mark.hidden', { effect: 'setEditMarkHidden', hidden });
          queueMark('setEditMarkHidden', () => setEditMarkHidden(apiBridge.client, hidden));
        },
        clearEditMark() {
          Logger.info('editor.edit_mark.clear', { effect: 'clearEditMark' });
          queueMark('clearEditMark', () => clearEditMark(apiBridge.client));
        },
      }),
      [sendMessage, apiBridge, queueMark]
    );

    const editorConfig = useMemo<EditorConfig>(
      () => ({
        initialText: isInitializedRef.current ? initialValueRef.current : value,
        isDark,
        fontSize,
        lineHeight,
        wordWrap,
        showWhitespace,
        language,
        cursorColor: palette.cursor,
        selectionColor: palette.selection,
        backgroundColor,
        gutterColor,
        borderColor,
        tabSize,
        ...(canvas ? { noAutoFocus: true, surface: 'canvas' as const, syntaxPalette } : {}),
      }),
      [
        fontSize,
        lineHeight,
        canvas,
        syntaxPalette,
        palette,
        backgroundColor,
        isDark,
        gutterColor,
        borderColor,
        tabSize,
        value,
        wordWrap,
        showWhitespace,
        language,
      ]
    );

    // Render-phase sync: before the WebView initializes, keep the pending initial text aligned with the latest React value
    if (!isInitializedRef.current) {
      appliedDocumentKey.current = documentKey;
      initialValueRef.current = value;
      lastWebViewTextRef.current = value;
      textMirror.set(value, false);
    }

    const initializeEditor = useCallback(() => {
      Logger.info('editor.initialize', { effect: 'initializeEditor' });
      if (!isReadyRef.current || isInitializedRef.current) {
        return;
      }

      isInitializedRef.current = true;
      appliedLanguageRef.current = editorConfig.language as Language;
      initLanguageRef.current = editorConfig.language as Language;
      updateHydrated(false);
      sendMessage({ type: 'init', config: editorConfig });
      if (!editorConfig.noAutoFocus) {
        sendMessage({ type: 'focus' });
      }
    }, [editorConfig, sendMessage, updateHydrated]);

    useEffect(() => {
      Logger.debug('editor.settings_sync', { effect: 'settingsSync' });
      if (!isInitializedRef.current) {
        return;
      }
      // Settings changed after the editor was initialized — update live.
      sendMessage({
        type: 'updateConfig',
        config: {
          isDark,
          fontSize,
          lineHeight,
          wordWrap,
          showWhitespace,
          cursorColor: palette.cursor,
          selectionColor: palette.selection,
          backgroundColor,
          ...(syntaxPalette ? { syntaxPalette } : {}),
          gutterColor,
          borderColor,
          tabSize,
        },
      });
    }, [
      paletteId,
      fontSize,
      wordWrap,
      showWhitespace,
      tabSize,
      sendMessage,
      isDark,
      lineHeight,
      palette.cursor,
      palette.selection,
      backgroundColor,
      syntaxPalette,
      gutterColor,
      borderColor,
    ]);

    // A language switch goes on the wire before the text write the screen makes for it: requests are answered in order, so the
    // page changes grammar first and the new starter lands under it.
    useEffect(() => {
      if (!isInitializedRef.current || appliedLanguageRef.current === language) return;
      appliedLanguageRef.current = language;
      sendMessage({ type: 'updateConfig', config: { language } });
    }, [language, sendMessage]);

    // The suggestion config goes to the page through the typed API, once the page has hydrated (a page that starts again forgets it) and
    // on every change of it after that.
    const sendCompletion = useCallback(() => {
      const wanted = completionRef.current ?? { enabled: false, entries: [] };
      const key = JSON.stringify(wanted);
      if (sentCompletionRef.current === key || (sentCompletionRef.current === null && !wanted.enabled)) return;
      sentCompletionRef.current = key;
      void apiBridge.client.setConfig({ completion: wanted }).then((reply) => {
        if (reply.ok) return;
        if (reply.error.code === 'transport' && reply.error.cause === 'closed' && !isInitializedRef.current) return;
        Logger.warn('editor.completion.config_failed', { code: reply.error.code });
      });
    }, [apiBridge]);
    const completionKey = JSON.stringify(completion ?? null);
    useEffect(() => {
      if (hydratedRef.current) sendCompletion();
    }, [completionKey, sendCompletion]);
    // Subscribed once the page has hydrated, and again when it hydrates anew: a subscription asked for before the page can answer is a request
    // that dies with the screen. The list is closed in a page that has just started.
    const completionListenerRef = useRef<(() => void) | null>(null);
    const listenForCompletion = useCallback(() => {
      completionListenerRef.current?.();
      completionOpenRef.current = false;
      completionListenerRef.current = apiBridge.client.on('completion', (event) => {
        completionOpenRef.current = event.open;
      });
    }, [apiBridge]);

    // When the store changes `value` from outside (e.g. replaceAll), push the
    // new text to CM6. Skip if CM6 was the source of the change to avoid loops.

    useEffect(() => {
      Logger.debug('editor.value_sync', { effect: 'valueSync' });
      if (!isInitializedRef.current) return;
      if (appliedDocumentKey.current !== documentKey) {
        appliedDocumentKey.current = documentKey;
        const loadDocument = async () => {
          if (!isReadyRef.current || appliedDocumentKey.current !== documentKey) return;
          const reply = await apiBridge.client.load(value);
          if (appliedDocumentKey.current !== documentKey) return;
          if (!reply.ok) {
            Logger.warn('editor.document.load_failed', { code: reply.error.code });
            return;
          }
          holdText(normalizeText(value));
          onUndoAvailabilityChange?.(false);
          onSelectionChange({ start: 0, end: 0 });
        };
        writeChainRef.current = writeChainRef.current.then(loadDocument, loadDocument);
        return;
      }
      if (value !== lastWebViewTextRef.current) {
        void writeDocument({ next: () => ({ text: valueRef.current }), label: 'react.value' });
      }
    }, [apiBridge, documentKey, holdText, onSelectionChange, onUndoAvailabilityChange, value, writeDocument]);

    const updateHydratedRef = useRef(updateHydrated);
    updateHydratedRef.current = updateHydrated;

    useEffect(() => {
      const timer = setTimeout(() => {
        if (hydratedRef.current) return;
        Logger.warn('editor.hydrate.timeout', {
          ready: isReadyRef.current,
          initialized: isInitializedRef.current,
          ms: HYDRATE_TIMEOUT_MS,
        });
      }, HYDRATE_TIMEOUT_MS);
      return () => clearTimeout(timer);
    }, []);

    const bridgeEpoch = useRef(0);
    useLayoutEffect(() => {
      const epoch = ++bridgeEpoch.current;
      Logger.debug('editor.cleanup', { effect: 'cleanup' });
      return () => {
        isReadyRef.current = false;
        isInitializedRef.current = false;
        queuedFocusRef.current = false;
        queuedSelectionRef.current = null;
        queuedTextRef.current = null;
        caretDragRef.current?.drag.cancel();
        caretDragRef.current = null;
        updateHydratedRef.current(false);
        queueMicrotask(() => {
          if (bridgeEpoch.current !== epoch) return;
          apiBridge.close();
          completionListenerRef.current?.();
          completionListenerRef.current = null;
        });
      };
    }, [apiBridge]);

    const logColdStart = () => {
      const report = coldStartRef.current?.take({ evalMs: pageEvalMsRef.current });
      if (report) Logger.info('editor.cold_start', report);
    };

    const handleEditorMessage = useCallback(
      (m: FromEditorMessage) => {
        switch (m.type) {
          case 'ready':
            Logger.info('editor.webview.ready', {});
            coldStartRef.current?.mark('ready');
            pageEvalMsRef.current = m.evalMs;
            isReadyRef.current = true;
            initializeEditor();
            break;
          case 'hydrated':
            holdText(m.text);
            onUndoAvailabilityChange?.(false);
            updateHydrated(true);
            coldStartRef.current?.mark('hydrated');
            logColdStart();
            // The view hydrated from whatever `initialText` was when it
            // mounted; React state may have moved since. Reconcile to state
            // -- a queued programmatic write first (it carries a selection),
            // otherwise the current value -- so the editor never shows code
            // other than what Run Local will run.
            if (queuedTextRef.current != null) {
              const { text, sel } = queuedTextRef.current;
              queuedTextRef.current = null;
              void writeDocument({ next: () => ({ text, selection: sel }), label: 'react.queued', focus: true });
            } else if (m.text !== valueRef.current) {
              Logger.info('editor.hydrated.reconciled', {
                viewLength: m.text.length,
                valueLength: valueRef.current.length,
              });
              void writeDocument({ next: () => ({ text: valueRef.current }), label: 'react.reconcile' });
            }
            sentCompletionRef.current = null;
            sendCompletion();
            listenForCompletion();
            // a switch made between init and hydration was dropped: the page was not ready for it
            if (languageRef.current !== initLanguageRef.current) {
              initLanguageRef.current = languageRef.current;
              sendMessage({ type: 'updateConfig', config: { language: languageRef.current } });
            }
            if (queuedSelectionRef.current != null) {
              const selection = queuedSelectionRef.current;
              queuedSelectionRef.current = null;
              sendMessage({
                type: 'setSelection',
                start: selection.start,
                end: selection.end,
              });
            } else {
              queuedSelectionRef.current = null;
            }
            if (queuedFocusRef.current) {
              queuedFocusRef.current = false;
              sendMessage({ type: 'focus' });
            } else {
              queuedFocusRef.current = false;
            }
            onSelectionChange(m.selection);
            break;
          case 'focused':
            onFocusChange?.(true);
            break;
          case 'blurred':
            onFocusChange?.(false);
            break;
          case 'log':
            Logger.debug('editor.webview.log', { message: m.message });
            break;
          case 'error':
            Logger.warn('editor.webview.js_error', {
              message: m.message.slice(0, 200),
            });
            break;
          case 'inputTiming':
            if (__DEV__) Logger.info('editor.input_timing', {
              inputToUpdateMs: m.inputToUpdateMs,
              updateToFrameMs: m.updateToFrameMs,
              updates: m.updates,
              frameToHostMs: Math.max(0, Date.now() - m.sentAtMs),
              nth: coldStartRef.current?.nth,
            });
            break;
          case 'contentChanged':
            holdText(m.text);
            onUndoAvailabilityChange?.(m.canUndo);
            onChangeText(m.text);
            onSelectionChange(m.selection);
            break;
          case 'selectionChanged':
            if (caretDragRef.current) caretDragRef.current.held = m.selection;
            else onSelectionChange(m.selection);
            break;
        }
      },
      [
        initializeEditor,
        onChangeText,
        onFocusChange,
        onSelectionChange,
        onUndoAvailabilityChange,
        listenForCompletion,
        sendCompletion,
        sendMessage,
        updateHydrated,
        writeDocument,
        holdText,
      ]
    );

    // Dispatched through a ref, not an effect subscription: the webview's one
    // 'ready' can arrive before effects run, and a dropped 'ready' means no init.
    const handleEditorMessageRef = useRef(handleEditorMessage);
    handleEditorMessageRef.current = handleEditorMessage;

    // The WebView's onMessage feeds this editor first, then any port subscribers.
    const handleMessage = useCallback(
      (event: WebViewMessageEvent) => {
        // A typed-API frame goes to the rpc client and never enters the old path, whose listeners expect a `type`.
        if (decode(event.nativeEvent.data).type !== 'foreign') {
          apiBridge.receive(event.nativeEvent.data);
          return;
        }
        let msg: FromEditorMessage;
        try {
          msg = JSON.parse(event.nativeEvent.data) as FromEditorMessage;
        } catch (err) {
          Logger.withError(err).debug('editor.message.parse_skip');
          return;
        }
        handleEditorMessageRef.current(msg);
        for (const listener of messageListenersRef.current) {
          listener(msg);
        }
      },
      [apiBridge]
    );

    return (
      <View
        testID={testID ?? 'CodeEditorContainer'}
        flex={1}
        overflow="hidden"
        rounded={canvas ? 0 : "$3"}
        style={{ backgroundColor }}
      >
        <WebView<object>
          testID="CodeEditorInput"
          ref={webViewRef}
          source={{ html }}
          onMessage={handleMessage}
          onLoadStart={() => {
            coldStartRef.current?.mark('loadStart');
            Logger.info('editor.webview.load_start', {});
          }}
          onLoadEnd={() => {
            Logger.info('editor.webview.load_end', {});
          }}
          onError={(e) => {
            Logger.warn('editor.webview.error', {
              desc: e.nativeEvent.description,
            });
          }}
          onHttpError={(e) => {
            Logger.warn('editor.webview.http_error', {
              statusCode: e.nativeEvent.statusCode,
            });
          }}
          onRenderProcessGone={(e) => {
            Logger.error('editor.webview.process_gone', {
              didCrash: e.nativeEvent.didCrash,
            });
          }}
          originWhitelist={['*']}
          javaScriptEnabled
          domStorageEnabled={false}
          bounces={false}
          scrollEnabled={false}
          keyboardDisplayRequiresUserAction={false}
          hideKeyboardAccessoryView
          allowsInlineMediaPlayback
          style={{ flex: 1, backgroundColor }}
        />
        {Platform.OS === 'android' ? <EditorTextMirror source={textMirror} /> : null}
      </View>
    );
  }
);
