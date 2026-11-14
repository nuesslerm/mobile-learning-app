import { Logger } from '@lib/logging';
import { View } from '@lib/tamagui-design-system';
import React, {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useMemo,
  useRef,
} from 'react';
import { Platform } from 'react-native';
import { WebView, type WebViewMessageEvent } from 'react-native-webview';

import type {
  CodeEditorPort,
  EditorConfig,
  FromEditorMessage,
  ToEditorMessage,
} from '../../domain/ports';
import { buildEditorHtml } from './editorHtml';
import {
  getCodeFont,
  getPalette,
  useCodeEditorSettingsStore,
} from './editorSettings';
import type { Selection } from './types';

// App default 400ms flush — same cadence as every other Logger.

// A cold phone WebView takes seconds to evaluate the CodeMirror bundle; past
// this without 'hydrated' the editor is dead or blank.
const HYDRATE_TIMEOUT_MS = 15_000;

export type EditorInputHandle = {
  focus: () => void;
  blur: () => void;
  applySelection: (sel: Selection) => void;
  insertAtCursor: (text: string, cursorOffset?: number) => void;
  /** Text as if typed at the caret: auto-closed brackets, re-indented `else:` and the rest of what typing does. */
  typeText: (text: string) => void;
  /** Replace the whole document and place the selection in one round-trip. */
  setTextAndSelection: (text: string, sel: Selection) => void;
  undo: () => void;
  redo: () => void;
  showDiff: (start: number, end: number) => void;
  hideDiff: () => void;
};

type Props = {
  value: string;
  onChangeText: (text: string) => void;
  onSelectionChange: (sel: Selection) => void;
  onHydratedChange?: (hydrated: boolean) => void;
  onFocusChange?: (focused: boolean) => void;
  tabSize?: number;
  testID?: string;
};

/**
 * CodeMirror 6 editor mounted inside a WebView.
 *
 * The HTML template is static; all configuration and edits flow over a
 * bidirectional postMessage protocol (see EditorMessages.ts). Adapted from the
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
      tabSize = 4,
      testID,
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
    const initialValueRef = useRef(value);
    // Tracks the last text CM6 reported via contentChanged, so we can detect
    // external store changes (e.g. replaceAll) and push them back to CM6.
    const lastWebViewTextRef = useRef(value);
    // Tracks whether we've sent the 'init' message to the editor.
    const isInitializedRef = useRef(false);

    const paletteId = useCodeEditorSettingsStore((s) => s.paletteId);
    const fontSize = useCodeEditorSettingsStore((s) => s.fontSize);
    const wordWrap = useCodeEditorSettingsStore((s) => s.wordWrap);
    const palette = getPalette(paletteId);
    const { lineHeight } = getCodeFont(fontSize);
    const isDark = palette.appearance === 'dark';
    // Editor chrome colors (gutter + border) are intentionally tied to the
    // editor palette's appearance, not the OS theme — they live inside the
    // embedded WebView, so the surrounding app's tokens don't apply.
    const gutterColor = isDark ? '#5c6370' : '#9aa0a6';
    const borderColor = isDark ? '#3e4451' : '#dadce0';

    // HTML is static — no config injected. All config flows via postMessage.
    const html = useMemo(() => buildEditorHtml(), []);

    // One CodeEditorPort per editor instance, bound to this WebView: post()
    // drives the injectJavaScript channel, onMessage() fans webview events out
    // to subscribers (the webview stub in flow tests routes through the same
    // shape).
    const messageListenersRef = useRef<Set<(m: FromEditorMessage) => void>>(
      new Set()
    );
    const editorPort = useMemo<CodeEditorPort>(
      () => ({
        post(msg: ToEditorMessage) {
          const webView = webViewRef.current;
          if (!webView) return;
          webView.injectJavaScript(
            `window.dispatchEvent(new MessageEvent('message', { data: ${JSON.stringify(JSON.stringify(msg))} })); true;`
          );
        },
        onMessage(listener) {
          messageListenersRef.current.add(listener);
          return () => {
            messageListenersRef.current.delete(listener);
          };
        },
      }),
      []
    );

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
          // Keep our mirror in sync so the value-sync effect doesn't echo the
          // change back as a redundant setText once React state catches up.
          lastWebViewTextRef.current = text;
          sendMessage({ type: 'setText', text });
          sendMessage({ type: 'setSelection', start: sel.start, end: sel.end });
        },
        undo() {
          Logger.info('editor.undo', { effect: 'undo' });
          sendMessage({ type: 'undo' });
        },
        redo() {
          Logger.info('editor.redo', { effect: 'redo' });
          sendMessage({ type: 'redo' });
        },
        showDiff(start: number, end: number) {
          // Clamp to the text the view has, not React's: a range past the view's end once left a
          // decoration it could not map, and React still holds the old text right after setTextAndSelection.
          const max = lastWebViewTextRef.current.length;
          const from = Math.max(0, Math.min(start, end, max));
          const to = Math.min(Math.max(start, end), max);
          Logger.info('editor.show_diff', { effect: 'showDiff', start: from, end: to });
          sendMessage({ type: 'showDiff', start: from, end: to });
        },
        hideDiff() {
          Logger.info('editor.hide_diff', { effect: 'hideDiff' });
          sendMessage({ type: 'hideDiff' });
        },
      }),
      [sendMessage]
    );

    const editorConfig = useMemo<EditorConfig>(
      () => ({
        initialText: isInitializedRef.current ? initialValueRef.current : value,
        isDark,
        fontSize,
        lineHeight,
        wordWrap,
        language: 'python',
        cursorColor: palette.cursor,
        selectionColor: palette.selection,
        backgroundColor: palette.background,
        gutterColor,
        borderColor,
        tabSize,
        noAutoFocus: false,
      }),
      [
        fontSize,
        lineHeight,
        palette,
        isDark,
        gutterColor,
        borderColor,
        tabSize,
        value,
        wordWrap,
      ]
    );

    // Render-phase sync: before the WebView initializes, keep the pending initial text aligned with the latest React value
    if (!isInitializedRef.current) {
      initialValueRef.current = value;
      lastWebViewTextRef.current = value;
    }

    const initializeEditor = useCallback(() => {
      Logger.info('editor.initialize', { effect: 'initializeEditor' });
      if (!isReadyRef.current || isInitializedRef.current) {
        return;
      }

      isInitializedRef.current = true;
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
          cursorColor: palette.cursor,
          selectionColor: palette.selection,
          backgroundColor: palette.background,
          gutterColor,
          borderColor,
          tabSize,
        },
      });
    }, [
      paletteId,
      fontSize,
      wordWrap,
      tabSize,
      sendMessage,
      isDark,
      lineHeight,
      palette.cursor,
      palette.selection,
      palette.background,
      gutterColor,
      borderColor,
    ]);

    // When the store changes `value` from outside (e.g. replaceAll), push the
    // new text to CM6. Skip if CM6 was the source of the change to avoid loops.

    useEffect(() => {
      Logger.debug('editor.value_sync', { effect: 'valueSync' });
      if (!isInitializedRef.current) return;
      if (value !== lastWebViewTextRef.current) {
        lastWebViewTextRef.current = value;
        sendMessage({ type: 'setText', text: value });
      }
    }, [value, sendMessage]);

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

    useLayoutEffect(() => {
      Logger.debug('editor.cleanup', { effect: 'cleanup' });
      return () => {
        isReadyRef.current = false;
        isInitializedRef.current = false;
        queuedFocusRef.current = false;
        queuedSelectionRef.current = null;
        queuedTextRef.current = null;
        updateHydratedRef.current(false);
      };
    }, []);

    const handleEditorMessage = useCallback(
      (m: FromEditorMessage) => {
        switch (m.type) {
          case 'ready':
            Logger.info('editor.webview.ready', {});
            isReadyRef.current = true;
            initializeEditor();
            break;
          case 'hydrated':
            lastWebViewTextRef.current = m.text;
            updateHydrated(true);
            // The view hydrated from whatever `initialText` was when it
            // mounted; React state may have moved since. Reconcile to state
            // -- a queued programmatic write first (it carries a selection),
            // otherwise the current value -- so the editor never shows code
            // other than what Run Local will run.
            if (queuedTextRef.current != null) {
              const { text, sel } = queuedTextRef.current;
              queuedTextRef.current = null;
              lastWebViewTextRef.current = text;
              sendMessage({ type: 'setText', text });
              queuedSelectionRef.current = sel;
            } else if (m.text !== valueRef.current) {
              Logger.info('editor.hydrated.reconciled', {
                viewLength: m.text.length,
                valueLength: valueRef.current.length,
              });
              lastWebViewTextRef.current = valueRef.current;
              sendMessage({ type: 'setText', text: valueRef.current });
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
          case 'contentChanged':
            lastWebViewTextRef.current = m.text;
            onChangeText(m.text);
            onSelectionChange(m.selection);
            break;
          case 'selectionChanged':
            onSelectionChange(m.selection);
            break;
        }
      },
      [
        initializeEditor,
        onChangeText,
        onFocusChange,
        onSelectionChange,
        sendMessage,
        updateHydrated,
      ]
    );

    // Dispatched through a ref, not an effect subscription: the webview's one
    // 'ready' can arrive before effects run, and a dropped 'ready' means no init.
    const handleEditorMessageRef = useRef(handleEditorMessage);
    handleEditorMessageRef.current = handleEditorMessage;

    // The WebView's onMessage feeds this editor first, then any port subscribers.
    const handleMessage = useCallback((event: WebViewMessageEvent) => {
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
    }, []);

    return (
      <View
        testID={testID ?? 'CodeEditorContainer'}
        flex={1}
        overflow="hidden"
        rounded="$3"
        style={{ backgroundColor: palette.background }}
      >
        <WebView
          testID="CodeEditorInput"
          ref={webViewRef}
          source={{ html }}
          onMessage={handleMessage}
          onLoadStart={() => {
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
          style={{ flex: 1, backgroundColor: palette.background }}
        />
      </View>
    );
  }
);
