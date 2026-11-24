/**
 * cm6Bundle.src.ts — Source for the CodeMirror 6 WebView bundle.
 *
 * Build with:
 *   pnpm --filter mla-tamagui run build:editor-bundle
 *
 * This file compiles to cm6Bundle.ts (the exported JS string) via esbuild.
 * Never edit cm6Bundle.ts directly — edit this file and rebuild.
 */

import { closeBrackets, closeBracketsKeymap } from '@codemirror/autocomplete';
import {
  history,
  defaultKeymap,
  historyKeymap,
  indentWithTab,
} from '@codemirror/commands';
import {
  indentOnInput,
  indentUnit,
  bracketMatching,
  syntaxHighlighting,
  defaultHighlightStyle,
  HighlightStyle,
} from '@codemirror/language';
import {
  Compartment,
  EditorSelection,
  EditorState,
  RangeSetBuilder,
} from '@codemirror/state';
import { oneDark, oneDarkTheme } from '@codemirror/theme-one-dark';
import {
  EditorView,
  keymap,
  lineNumbers,
  Decoration,
  highlightWhitespace,
  ViewPlugin,
  type ViewUpdate,
} from '@codemirror/view';
import {
  createDeferredEngine,
  createEditorEngine,
  isLanguage,
  LANGUAGE_NAMES,
  serveEditor,
  type CompletionConfig,
  type ConfigPatch,
  type EditorEngine,
  type EngineCore,
  type Language,
  type SelRange,
} from '@lib/code-editor-api';
import { tags } from '@lezer/highlight';
import { Logger } from '@lib/logging';
import { createPageTransport } from '@lib/webview-rpc';
import { createCm6Host, revisionField, selectionOf, setRevisionBase, typingCalls } from './cm6Host';
import {
  toEditorConfig,
  toLegacyPatch,
  type LegacyEditorConfig,
} from '../../../domain/editor/configPatch';
import { marksField } from './cm6Marks';
import { completionExtensions } from './completionExtension';
import { languageCompartment, languageExtension } from './languageExtensions';
declare global {
  type ReactNativeWebViewBridge = {
    postMessage: (msg: string) => void;
  };
  interface Window {
    ReactNativeWebView?: ReactNativeWebViewBridge;
    /** the typed editor API: the same engine the host reaches over the rpc channel */
    __editorEngine__?: EditorEngine;
    /** set by the page html in tests only */
    __EDITOR_DEBUG__?: boolean;
    __editorDebug__?: {
      simulateUser(
        spec:
          | { kind: 'type'; text: string }
          | { kind: 'select'; anchor: number; head?: number }
      ): void;
    };
  }
}

// ─── Snippet field marker characters (must match triggerEngine.ts) ────────────
const FIELD_START = '\uE001';
const FIELD_END = '\uE002';

/** Hides the invisible FIELD_START / FIELD_END marker characters. */
const fieldMarkerDecoration = Decoration.replace({});
function buildFieldMarkers(view: EditorView) {
  const builder = new RangeSetBuilder<Decoration>();
  const text = view.state.doc.toString();
  for (let i = 0; i < text.length; i++) {
    if (text[i] === FIELD_START || text[i] === FIELD_END) {
      builder.add(i, i + 1, fieldMarkerDecoration);
    }
  }
  return builder.finish();
}
const fieldMarkerPlugin = ViewPlugin.fromClass(
  class {
    decorations;
    constructor(view: EditorView) {
      Logger.info('cm6.constructor', {
        method: 'constructor',
      });
      this.decorations = buildFieldMarkers(view);
    }
    update(update: ViewUpdate) {
      Logger.info('cm6.update', {
        method: 'update',
      });
      if (update.docChanged || update.viewportChanged) {
        this.decorations = buildFieldMarkers(update.view);
      }
    }
  },
  {
    decorations: (v) => {
      Logger.info('cm6.decorations', {
        method: 'decorations',
      });
      return v.decorations;
    },
  }
);
type EditorConfig = LegacyEditorConfig;
type MessagePayload = Record<string, unknown>;
function isRecord(value: unknown): value is MessagePayload {
  return typeof value === 'object' && value !== null;
}
function getStringProp(payload: MessagePayload, key: string) {
  const value = payload[key];
  return typeof value === 'string' ? value : undefined;
}
function getNumberProp(payload: MessagePayload, key: string) {
  const value = payload[key];
  return typeof value === 'number' ? value : undefined;
}
function parseEditorConfig(value: unknown): EditorConfig | null {
  if (!isRecord(value)) {
    return null;
  }
  const initialText = getStringProp(value, 'initialText');
  const fontSize = getNumberProp(value, 'fontSize');
  const lineHeight = getNumberProp(value, 'lineHeight');
  const cursorColor = getStringProp(value, 'cursorColor');
  const selectionColor = getStringProp(value, 'selectionColor');
  const backgroundColor = getStringProp(value, 'backgroundColor');
  const gutterColor = getStringProp(value, 'gutterColor');
  const borderColor = getStringProp(value, 'borderColor');
  const tabSize = getNumberProp(value, 'tabSize');
  if (
    initialText === undefined ||
    typeof value.isDark !== 'boolean' ||
    fontSize === undefined ||
    lineHeight === undefined ||
    typeof value.wordWrap !== 'boolean' ||
    typeof value.language !== 'string' ||
    cursorColor === undefined ||
    selectionColor === undefined ||
    backgroundColor === undefined ||
    gutterColor === undefined ||
    borderColor === undefined ||
    tabSize === undefined
  ) {
    return null;
  }
  return {
    initialText,
    noAutoFocus: value.noAutoFocus === true,
    syntaxPalette: value.syntaxPalette === 'contrast' ? 'contrast' : 'default',
    isDark: value.isDark,
    fontSize,
    lineHeight,
    wordWrap: value.wordWrap,
    showWhitespace: value.showWhitespace === true,
    language: value.language,
    cursorColor,
    selectionColor,
    backgroundColor,
    gutterColor,
    borderColor,
    tabSize,
  };
}
function getMessagePayload(data: unknown): MessagePayload | null {
  if (typeof data !== 'string') {
    return null;
  }
  try {
    const parsed: unknown = JSON.parse(data);
    return isRecord(parsed) ? parsed : null;
    // eslint-disable-next-line mobile-learn/no-unhandled-catch -- this bundle runs inside the WebView and cannot import the app logger; a non-JSON message is simply not ours
  } catch {
    return null;
  }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

function postToRN(msg: Record<string, unknown>) {
  if (window.ReactNativeWebView) {
    window.ReactNativeWebView.postMessage(JSON.stringify(msg));
  }
}
function disableTextAssist(target: HTMLElement) {
  target.spellcheck = false;
  target.setAttribute('spellcheck', 'false');
  target.setAttribute('autocorrect', 'off');
  target.setAttribute('autocomplete', 'off');
  target.setAttribute('autocapitalize', 'off');
  target.setAttribute('data-gramm', 'false');
  target.setAttribute('data-gramm_editor', 'false');
  target.setAttribute('data-enable-grammarly', 'false');
}
function getEditorState(view: EditorView) {
  const sel = view.state.selection.main;
  return {
    cursor: sel.head,
    selection: {
      start: sel.from,
      end: sel.to,
    },
  };
}

// ─── Extension builder (recreated on config changes via Compartment) ──────────
const configCompartment = new Compartment();
// the completion the typed API configured; off until it says otherwise, and kept across a load and a language switch
const completionCompartment = new Compartment();
let currentCompletion: CompletionConfig = { enabled: false, entries: [] };
const completionFor = (language: string) =>
  completionExtensions({
    language: languageOf(language),
    enabled: currentCompletion.enabled,
    entries: currentCompletion.entries,
    onOpenChange: (open) => core?.ingestCompletion(open),
  });
const contrastHighlight = HighlightStyle.define([
  { tag: [tags.keyword, tags.definitionKeyword, tags.controlKeyword], color: '#FF3B91' },
  { tag: [tags.className, tags.typeName], color: '#65DCC3' },
  { tag: tags.function(tags.variableName), color: '#52C7B0' },
  { tag: [tags.null, tags.bool, tags.number], color: '#AC64FA' },
  { tag: [tags.string, tags.regexp], color: '#9ACC78' },
  { tag: [tags.comment, tags.docComment], color: '#6A7584' },
  { tag: [tags.punctuation, tags.operator], color: '#A0ABB9' },
]);

function buildConfigExtensions(cfg: EditorConfig) {
  const contrast = cfg.isDark && cfg.syntaxPalette === 'contrast';
  const theme = EditorView.theme(
    {
      '&': {
        fontSize: `${cfg.fontSize}px`,
        lineHeight: `${cfg.lineHeight}px`,
        fontFamily: "'Menlo', 'Courier New', monospace",
        height: '100%',
        backgroundColor: cfg.backgroundColor,
        ...(contrast ? { color: '#F1F1F4' } : {}),
      },
      '.cm-content': {
        padding: '12px 0',
        caretColor: cfg.cursorColor,
      },
      '.cm-line': {
        padding: '0 12px',
      },
      '.cm-scroller': {
        fontFamily: "'Menlo', 'Courier New', monospace",
      },
      '.cm-cursor': {
        borderLeftColor: cfg.cursorColor,
        borderLeftWidth: '2px',
      },
      '.cm-selectionBackground': {
        background: `${cfg.selectionColor} !important`,
      },
      '.cm-activeLine': {
        background: 'transparent',
      },
      '.cm-gutters': {
        background: cfg.backgroundColor,
        borderRight: `1px solid ${cfg.borderColor}`,
        color: cfg.gutterColor,
      },
      '.cm-lineNumbers .cm-gutterElement': {
        padding: '0 8px 0 4px',
        minWidth: '28px',
      },
      '&.cm-focused': {
        outline: 'none',
      },
      '.cm-matchingBracket': {
        background: cfg.isDark ? 'rgba(255,255,255,0.1)' : 'rgba(0,0,0,0.1)',
        borderBottom: `1px solid ${cfg.cursorColor}`,
      },
      '.cm-highlightSpace': {
        backgroundImage: `radial-gradient(circle at 50% 55%, ${cfg.isDark ? '#8b93a1' : '#9aa0a6'} 20%, transparent 5%)`,
      },
      '.cm-run-error-line': {
        background: cfg.isDark ? 'rgba(248,113,113,0.2)' : 'rgba(220,38,38,0.12)',
      },
      '.cm-run-error': {
        textDecoration: `underline wavy ${cfg.isDark ? '#f87171' : '#dc2626'}`,
        textUnderlineOffset: '3px',
      },
    },
    {
      dark: cfg.isDark,
    }
  );
  return [
    EditorState.tabSize.of(cfg.tabSize),
    // Spaces, as in every LeetCode stub: Python rejects a tab-indented line under a space-indented one (TabError).
    indentUnit.of(' '.repeat(cfg.tabSize)),
    theme,
    cfg.isDark ? (contrast ? [oneDarkTheme, syntaxHighlighting(contrastHighlight)] : oneDark) : [],
    cfg.wordWrap ? EditorView.lineWrapping : [],
    cfg.showWhitespace ? highlightWhitespace() : [],
  ];
}

// ─── Editor instance ──────────────────────────────────────────────────────────
let view: EditorView | null = null;
let currentConfig: EditorConfig | null = null;
let core: EngineCore | null = null;
let host: ReturnType<typeof createCm6Host> | null = null;
/** The editor language the page was told, or python for one it does not know (and says so once). */
function languageOf(id: string): Language {
  if (isLanguage(id)) return id;
  Logger.warn('editor.init.unknown_language', { language: id });
  return 'python';
}
const randomId = () =>
  Date.now().toString(36) + Math.random().toString(36).slice(2);
function buildState(cfg: EditorConfig, doc: string, selection?: SelRange) {
  const updateListener = EditorView.updateListener.of((update) => {
    if (update.docChanged) {
      postToRN({
        type: 'contentChanged',
        text: update.state.doc.toString(),
        ...getEditorState(update.view),
      });
    }
    if (update.selectionSet) {
      postToRN({
        type: 'selectionChanged',
        ...getEditorState(update.view),
      });
    }
  });
  const apiListener = EditorView.updateListener.of((update) => {
    try {
      host?.sync();
      if (!update.docChanged && update.selectionSet) {
        core?.ingestSelection(selectionOf(update.state));
      }
    } catch (error) {
      Logger.withError(error).warn('editor.api.ingest_failed');
    }
  });
  const touchHandler = EditorView.domEventHandlers({
    focus: () => {
      Logger.info('cm6.focus', {
        method: 'focus',
      });
      core?.ingestFocus(true);
      postToRN({
        type: 'focused',
      });
    },
    blur: () => {
      Logger.info('cm6.blur', {
        method: 'blur',
      });
      core?.ingestFocus(false);
      postToRN({
        type: 'blurred',
      });
    },
    touchend: () => {
      Logger.info('cm6.touchend', {
        method: 'touchend',
      });
      setTimeout(() => {
        if (view) {
          postToRN({
            type: 'selectionChanged',
            ...getEditorState(view),
          });
        }
      }, 50);
    },
  });
  return EditorState.create({
    doc,
    ...(selection
      ? { selection: EditorSelection.single(selection.anchor, selection.head) }
      : {}),
    extensions: [
      lineNumbers(),
      history(),
      indentOnInput(),
      bracketMatching(),
      closeBrackets(),
      syntaxHighlighting(defaultHighlightStyle, {
        fallback: true,
      }),
      languageCompartment.of(languageExtension(languageOf(cfg.language))),
      completionCompartment.of(completionFor(cfg.language)),
      fieldMarkerPlugin,
      marksField,
      revisionField,
      typingCalls,
      configCompartment.of(buildConfigExtensions(cfg)),
      keymap.of([
        ...defaultKeymap,
        ...historyKeymap,
        ...closeBracketsKeymap,
        indentWithTab,
      ]),
      updateListener,
      apiListener,
      touchHandler,
    ],
  });
}
/** The one way a config patch lands: the language moves in its own compartment, in the same dispatch as the rest. */
function applyConfigPatch(patch: Partial<EditorConfig>) {
  if (!view || !currentConfig) return;
  const before = currentConfig.language;
  const { language, ...rest } = patch;
  const next = language !== undefined && isLanguage(language) ? language : before;
  currentConfig = { ...currentConfig, ...rest, language: next };
  const effects = [configCompartment.reconfigure(buildConfigExtensions(currentConfig))];
  if (next !== before) {
    effects.push(languageCompartment.reconfigure(languageExtension(languageOf(next))));
    effects.push(completionCompartment.reconfigure(completionFor(next)));
    Logger.info('editor.language.set', { from: before, to: next });
  }
  view.dispatch({ effects });
}
/** Applies a typed-API config patch. */
function applyApiConfig(patch: ConfigPatch) {
  if (!view || !currentConfig) {
    return {
      ok: false as const,
      error: { code: 'not-ready' as const, message: 'the editor has not been created yet' },
    };
  }
  applyConfigPatch(toLegacyPatch(patch));
  if (patch.completion) {
    currentCompletion = patch.completion;
    view.dispatch({ effects: completionCompartment.reconfigure(completionFor(currentConfig.language)) });
  }
  return { ok: true as const, config: toEditorConfig(currentConfig, currentCompletion) };
}
function createEditor(cfg: EditorConfig) {
  view?.destroy();
  currentConfig = cfg;
  setRevisionBase(0);
  view = new EditorView({
    state: buildState(cfg, cfg.initialText),
    parent: (() => {
      const editorRoot = document.getElementById('editor');
      if (!editorRoot) {
        throw new Error('Missing editor root');
      }
      return editorRoot;
    })(),
  });
  host = createCm6Host({
    view: () => view as EditorView,
    core: () => core,
    languages: () => LANGUAGE_NAMES,
    language: () => (currentConfig && isLanguage(currentConfig.language) ? currentConfig.language : 'python'),
    configure: applyApiConfig,
    buildState: (text, selection) =>
      buildState(currentConfig as EditorConfig, text, selection),
  });
  core = createEditorEngine(host, {
    instance: randomId(),
    logDepth: 256,
    logError: (error) => Logger.withError(error).warn('editor.api.internal'),
  });
  deferredEngine.bind(core);
  disableTextAssist(document.documentElement);
  disableTextAssist(document.body);
  disableTextAssist(view.contentDOM);
  if (!cfg.noAutoFocus) view.focus();
  postToRN({
    type: 'hydrated',
    text: view.state.doc.toString(),
    ...getEditorState(view),
  });
}

// ─── Typed editor API ─────────────────────────────────────────────────────────
// Served beside the old messages on the same channel: its rpc frames carry `v` and `kind` and no `type`, so the switch below ignores them.
const deferredEngine = createDeferredEngine((error) =>
  Logger.withError(error).warn('editor.api.internal')
);
serveEditor(deferredEngine, createPageTransport(window), (error) =>
  Logger.withError(error).warn('editor.api.serve_failed')
);
window.__editorEngine__ = deferredEngine;

// ─── Message handler ──────────────────────────────────────────────────────────
window.addEventListener('message', (event) => {
  const msg = getMessagePayload(event.data);
  if (!msg) {
    return;
  }
  switch (msg.type) {
    case 'init': {
      const config = parseEditorConfig(msg.config);
      if (!config) {
        return;
      }
      createEditor(config);
      break;
    }
    case 'blur':
      view?.contentDOM.blur();
      break;
  }
});

// ─── Test hook (page html sets __EDITOR_DEBUG__ in tests only) ─────────────────
// What a person's touch does, for the contract suite: it never exposes the view.
if (window.__EDITOR_DEBUG__) {
  window.__editorDebug__ = {
    simulateUser(spec) {
      if (!view) return;
      if (spec.kind === 'type') {
        view.dispatch(
          view.state.update(view.state.replaceSelection(spec.text), {
            userEvent: 'input.type',
          })
        );
      } else {
        view.dispatch({
          selection: EditorSelection.single(spec.anchor, spec.head ?? spec.anchor),
          userEvent: 'select.pointer',
        });
      }
    },
  };
}
