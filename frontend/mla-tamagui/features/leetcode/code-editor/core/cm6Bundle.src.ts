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
  undo,
  redo,
} from '@codemirror/commands';
import { cpp } from '@codemirror/lang-cpp';
import { python } from '@codemirror/lang-python';
import {
  indentOnInput,
  indentUnit,
  bracketMatching,
  syntaxHighlighting,
  defaultHighlightStyle,
} from '@codemirror/language';
import {
  Compartment,
  EditorState,
  RangeSetBuilder,
  StateEffect,
} from '@codemirror/state';
import { oneDark } from '@codemirror/theme-one-dark';
import {
  EditorView,
  keymap,
  lineNumbers,
  Decoration,
  type DecorationSet,
  ViewPlugin,
  type ViewUpdate,
} from '@codemirror/view';
import { Logger } from '@lib/logging';
declare global {
  type ReactNativeWebViewBridge = {
    postMessage: (msg: string) => void;
  };
  type EditorHandle = {
    getText(): string;
    getSelection(): {
      start: number;
      end: number;
    } | null;
    setSelection(start: number, end: number): void;
    insertText(text: string, offset?: number): void;
    replaceSelection(text: string): void;
    setText(text: string): void;
    focus(): void;
    blur(): void;
    undo(): void;
    redo(): void;
  };

  interface Window {
    ReactNativeWebView?: ReactNativeWebViewBridge;
    __editor__?: EditorHandle;
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
/**
 * A mark over [from, to], clamped to the document. React sends ranges from
 * its own copy of the text, which can be ahead of or behind the view (a
 * reset, a lost message); a decoration past the end of the document made
 * every later `dispatch` throw a RangeError, and the editor silently stopped
 * taking text. The plugins also map their marks through every change now.
 */
function markRange(view: EditorView, from: number, to: number, mark: Decoration): DecorationSet {
  const max = view.state.doc.length;
  const start = Math.max(0, Math.min(from, to, max));
  const end = Math.min(Math.max(from, to), max);
  if (end <= start) return Decoration.none;
  const builder = new RangeSetBuilder<Decoration>();
  builder.add(start, end, mark);
  return builder.finish();
}
const HIGHLIGHT_FLASH_DURATION = 3000;
const highlightMark = Decoration.mark({
  class: 'cm-highlight-flash',
});
const highlightPlugin = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet = Decoration.none;
    private timeout: ReturnType<typeof setTimeout> | null = null;
    addHighlight(
      view: EditorView,
      from: number,
      to: number,
      durationMs = HIGHLIGHT_FLASH_DURATION
    ) {
      Logger.info('cm6.addHighlight', {
        method: 'addHighlight',
      });
      this.decorations = markRange(view, from, to, highlightMark);
      view.dispatch({
        effects: StateEffect.appendConfig.of([]),
      });
      if (this.timeout) clearTimeout(this.timeout);
      this.timeout = setTimeout(() => {
        this.decorations = Decoration.none;
        view.dispatch({
          effects: StateEffect.appendConfig.of([]),
        });
      }, durationMs);
    }
    clear() {
      Logger.info('cm6.clear', {
        method: 'clear',
      });
      if (this.timeout) clearTimeout(this.timeout);
      this.decorations = Decoration.none;
    }
    update(update: ViewUpdate) {
      if (update.docChanged) this.decorations = this.decorations.map(update.changes);
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
const diffMark = Decoration.mark({
  class: 'cm-diff-change',
});
const diffPlugin = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet = Decoration.none;
    showDiff(view: EditorView, from: number, to: number) {
      Logger.info('cm6.showDiff', {
        method: 'showDiff',
      });
      this.decorations = markRange(view, from, to, diffMark);
      view.dispatch({
        effects: StateEffect.appendConfig.of([]),
      });
    }
    hideDiff() {
      Logger.info('cm6.hideDiff', {
        method: 'hideDiff',
      });
      this.decorations = Decoration.none;
    }
    update(update: ViewUpdate) {
      if (update.docChanged) this.decorations = this.decorations.map(update.changes);
    }
  },
  {
    decorations: (v) => {
      Logger.info('cm6.diffDecorations', {
        method: 'diffDecorations',
      });
      return v.decorations;
    },
  }
);
type EditorConfig = {
  initialText: string;
  isDark: boolean;
  fontSize: number;
  lineHeight: number;
  wordWrap: boolean;
  language: string;
  cursorColor: string;
  selectionColor: string;
  backgroundColor: string;
  gutterColor: string;
  borderColor: string;
  tabSize: number;
};
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
    isDark: value.isDark,
    fontSize,
    lineHeight,
    wordWrap: value.wordWrap,
    language: value.language,
    cursorColor,
    selectionColor,
    backgroundColor,
    gutterColor,
    borderColor,
    tabSize,
  };
}
function parseEditorConfigPatch(value: unknown): Partial<EditorConfig> | null {
  if (!isRecord(value)) {
    return null;
  }
  const patch: Partial<EditorConfig> = {};
  const initialText = getStringProp(value, 'initialText');
  const fontSize = getNumberProp(value, 'fontSize');
  const lineHeight = getNumberProp(value, 'lineHeight');
  const cursorColor = getStringProp(value, 'cursorColor');
  const selectionColor = getStringProp(value, 'selectionColor');
  const backgroundColor = getStringProp(value, 'backgroundColor');
  const gutterColor = getStringProp(value, 'gutterColor');
  const borderColor = getStringProp(value, 'borderColor');
  const tabSize = getNumberProp(value, 'tabSize');
  if (initialText !== undefined) patch.initialText = initialText;
  if (typeof value.isDark === 'boolean') patch.isDark = value.isDark;
  if (fontSize !== undefined) patch.fontSize = fontSize;
  if (lineHeight !== undefined) patch.lineHeight = lineHeight;
  if (typeof value.wordWrap === 'boolean') patch.wordWrap = value.wordWrap;
  if (typeof value.language === 'string') patch.language = value.language;
  if (cursorColor !== undefined) patch.cursorColor = cursorColor;
  if (selectionColor !== undefined) patch.selectionColor = selectionColor;
  if (backgroundColor !== undefined) patch.backgroundColor = backgroundColor;
  if (gutterColor !== undefined) patch.gutterColor = gutterColor;
  if (borderColor !== undefined) patch.borderColor = borderColor;
  if (tabSize !== undefined) patch.tabSize = tabSize;
  return Object.keys(patch).length > 0 ? patch : null;
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
function buildConfigExtensions(cfg: EditorConfig) {
  const theme = EditorView.theme(
    {
      '&': {
        fontSize: `${cfg.fontSize}px`,
        lineHeight: `${cfg.lineHeight}px`,
        fontFamily: "'Menlo', 'Courier New', monospace",
        height: '100%',
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
    },
    {
      dark: cfg.isDark,
    }
  );
  return [
    EditorState.tabSize.of(cfg.tabSize),
    // Spaces, as in every LeetCode stub: Python rejects a tab-indented line under a space-indented one (TabError).
    indentUnit.of(' '.repeat(cfg.tabSize)),
    cfg.isDark ? oneDark : [],
    theme,
    cfg.wordWrap ? EditorView.lineWrapping : [],
  ];
}

// ─── Editor instance ──────────────────────────────────────────────────────────
let view: EditorView | null = null;
let currentConfig: EditorConfig | null = null;
function createEditor(cfg: EditorConfig) {
  currentConfig = cfg;
  const lang = cfg.language === 'python' ? python() : cpp();
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
  const touchHandler = EditorView.domEventHandlers({
    focus: () => {
      Logger.info('cm6.focus', {
        method: 'focus',
      });
      postToRN({
        type: 'focused',
      });
    },
    blur: () => {
      Logger.info('cm6.blur', {
        method: 'blur',
      });
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
  const state = EditorState.create({
    doc: cfg.initialText,
    extensions: [
      lineNumbers(),
      history(),
      indentOnInput(),
      bracketMatching(),
      closeBrackets(),
      syntaxHighlighting(defaultHighlightStyle, {
        fallback: true,
      }),
      lang,
      fieldMarkerPlugin,
      highlightPlugin,
      diffPlugin,
      configCompartment.of(buildConfigExtensions(cfg)),
      keymap.of([
        ...defaultKeymap,
        ...historyKeymap,
        ...closeBracketsKeymap,
        indentWithTab,
      ]),
      updateListener,
      touchHandler,
    ],
  });
  view = new EditorView({
    state,
    parent: (() => {
      const editorRoot = document.getElementById('editor');
      if (!editorRoot) {
        throw new Error('Missing editor root');
      }
      return editorRoot;
    })(),
  });
  disableTextAssist(document.documentElement);
  disableTextAssist(document.body);
  disableTextAssist(view.contentDOM);
  view.focus();
  postToRN({
    type: 'hydrated',
    text: view.state.doc.toString(),
    ...getEditorState(view),
  });
}

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
    case 'updateConfig':
      if (view && currentConfig) {
        const configPatch = parseEditorConfigPatch(msg.config);
        if (!configPatch) {
          return;
        }
        currentConfig = {
          ...currentConfig,
          ...configPatch,
        };
        view.dispatch({
          effects: configCompartment.reconfigure(
            buildConfigExtensions(currentConfig)
          ),
        });
      }
      break;
    case 'setText':
      if (view) {
        const text = getStringProp(msg, 'text');
        if (text === undefined) {
          return;
        }
        view.dispatch({
          changes: {
            from: 0,
            to: view.state.doc.length,
            insert: text,
          },
        });
        postToRN({
          type: 'hydrated',
          text: view.state.doc.toString(),
          ...getEditorState(view),
        });
      }
      break;
    case 'insertAt': {
      if (!view) break;
      const pos = getNumberProp(msg, 'pos') ?? view.state.selection.main.head;
      const text = getStringProp(msg, 'text') ?? '';
      const offset = getNumberProp(msg, 'offset') ?? 0;
      view.dispatch({
        changes: {
          from: pos,
          to: pos,
          insert: text,
        },
        selection: {
          anchor: pos + text.length + offset,
        },
      });
      break;
    }
    case 'typeText': {
      if (!view) break;
      const editor = view;
      const text = getStringProp(msg, 'text') ?? '';
      const { from, to } = editor.state.selection.main;
      // The path a typed key takes: input handlers (closeBrackets) first, then an insert marked as typing,
      // which indentOnInput re-indents.
      const insert = () =>
        editor.state.update(editor.state.replaceSelection(text), {
          scrollIntoView: true,
          userEvent: 'input.type',
        });
      const handled = editor.state
        .facet(EditorView.inputHandler)
        .some((handler) => handler(editor, from, to, text, insert));
      if (!handled) editor.dispatch(insert());
      break;
    }
    case 'replaceSelection': {
      if (!view) break;
      const sel = view.state.selection.main;
      const text = getStringProp(msg, 'text') ?? '';
      view.dispatch({
        changes: {
          from: sel.from,
          to: sel.to,
          insert: text,
        },
        selection: {
          anchor: sel.from + text.length,
        },
      });
      break;
    }
    case 'setSelection':
      if (view) {
        const start = getNumberProp(msg, 'start');
        if (start === undefined) {
          return;
        }
        const end = getNumberProp(msg, 'end') ?? start;
        view.dispatch({
          selection: {
            anchor: start,
            head: end,
          },
        });
        view.focus();
      }
      break;
    case 'focus':
      view?.focus();
      break;
    case 'blur':
      view?.contentDOM.blur();
      break;
    case 'undo':
      if (view) undo(view);
      break;
    case 'redo':
      if (view) redo(view);
      break;
    case 'getState':
      if (view) {
        postToRN({
          type: 'stateResponse',
          requestId: msg.requestId,
          text: view.state.doc.toString(),
          ...getEditorState(view),
        });
      }
      break;
    case 'highlightRange': {
      if (!view) break;
      const start = getNumberProp(msg, 'start');
      const end = getNumberProp(msg, 'end');
      const durationMs = getNumberProp(msg, 'durationMs');
      if (start !== undefined && end !== undefined) {
        const plugin = view.plugin(highlightPlugin);
        plugin?.addHighlight(view, start, end, durationMs ?? undefined);
      }
      break;
    }
    case 'clearHighlights': {
      if (!view) break;
      const plugin = view.plugin(highlightPlugin);
      plugin?.clear();
      break;
    }
    case 'showDiff': {
      if (!view) break;
      const start = getNumberProp(msg, 'start');
      const end = getNumberProp(msg, 'end');
      if (start !== undefined && end !== undefined) {
        const plugin = view.plugin(diffPlugin);
        plugin?.showDiff(view, start, end);
      }
      break;
    }
    case 'hideDiff': {
      if (!view) break;
      const plugin = view.plugin(diffPlugin);
      plugin?.hideDiff();
      break;
    }
  }
});

// ─── Web test driver ──────────────────────────────────────────────────────────
// Expose editor handle for Playwright / web E2E so web tests share the same
// imperative API as mobile (editorRef.current.*).
window.__editor__ = {
  getText() {
    Logger.info('cm6.getText', {
      method: 'getText',
    });
    return view?.state.doc.toString() ?? '';
  },
  getSelection() {
    Logger.info('cm6.getSelection', {
      method: 'getSelection',
    });
    if (!view) return null;
    const sel = view.state.selection.main;
    return {
      start: sel.from,
      end: sel.to,
    };
  },
  setSelection(start: number, end: number) {
    Logger.info('cm6.setSelection', {
      method: 'setSelection',
    });
    if (!view) return;
    view.dispatch({
      selection: {
        anchor: start,
        head: end,
      },
    });
    view.focus();
  },
  insertText(text: string, offset?: number) {
    Logger.info('cm6.insertText', {
      method: 'insertText',
    });
    if (!view) return;
    const pos = view.state.selection.main.head;
    view.dispatch({
      changes: {
        from: pos,
        to: pos,
        insert: text,
      },
      selection: {
        anchor: pos + text.length + (offset ?? 0),
      },
    });
  },
  replaceSelection(text: string) {
    Logger.info('cm6.replaceSelection', {
      method: 'replaceSelection',
    });
    if (!view) return;
    const sel = view.state.selection.main;
    view.dispatch({
      changes: {
        from: sel.from,
        to: sel.to,
        insert: text,
      },
      selection: {
        anchor: sel.from + text.length,
      },
    });
  },
  setText(text: string) {
    Logger.info('cm6.setText', {
      method: 'setText',
    });
    if (!view) return;
    view.dispatch({
      changes: {
        from: 0,
        to: view.state.doc.length,
        insert: text,
      },
    });
  },
  focus() {
    Logger.info('cm6.focus', {
      method: 'focus',
    });
    view?.focus();
  },
  blur() {
    Logger.info('cm6.blur', {
      method: 'blur',
    });
    view?.contentDOM.blur();
  },
  undo() {
    Logger.info('cm6.undo', {
      method: 'undo',
    });
    if (view) undo(view);
  },
  redo() {
    Logger.info('cm6.redo', {
      method: 'redo',
    });
    if (view) redo(view);
  },
};
