/**
 * Registry of the built-in coding keys: display metadata (icon/glyph + the
 * human name shown on the config screen) and the action each one runs against
 * the editor.
 *
 * Actions are pure-ish closures over a {@link KeyActionContext} (the editor
 * handle + current text/selection), reusing the transforms in ./editing. The
 * editor handle is imported type-only, so importing this registry (e.g. from the
 * config screen, which only needs the metadata) does not pull in the editor
 * WebView bundle.
 */

import type { BuiltinKeyId } from '../../domain/index';
import { Logger } from '@lib/logging';
import type { LucideIcon } from 'lucide-react-native';
import {
  ArrowLeft,
  ArrowLeftToLine,
  ArrowRight,
  ArrowRightToLine,
  Braces,
  Brackets,
  CornerDownLeft,
  Hash,
  Parentheses,
  RotateCcw,
  RotateCw,
} from '@lib/tamagui-design-system/icons';
import type { Language } from '@lib/code-editor-api';
import type { RefObject } from 'react';

import type { EditorInputHandle } from './EditorInput';
import { profileOf, type LanguageProfile } from '../../domain/editor/languageProfile';
import {
  dedent,
  indent,
  moveLeft,
  moveRight,
  smartNewline,
  toggleComment,
  type EditResult,
} from '../../domain/editor/editing';
import type { Selection } from '../../domain/editor/types';
export type KeyActionContext = {
  editorRef: RefObject<EditorInputHandle | null>;
  text: string;
  selection: Selection;
  /** The language the editor is in: the Comment and Newline keys follow it. */
  language: Language;
  /** One indentation level: as many spaces as the person's Tabs preference. */
  indentUnit: string;
};
export type BuiltinKeyDef = {
  /** Human-readable name shown on the configure screen. */
  name: string;
  /** Either an icon or a glyph label is shown on the key (never both). */
  icon?: LucideIcon;
  label?: string;
  run: (ctx: KeyActionContext) => void;
};
function pairRun(open: string) {
  return (ctx: KeyActionContext): void => {
    // Typed, not inserted: the editor's bracket handling adds the pair (a plain insert left typing the closer to add a second one)
    // or wraps a selection and keeps the inner text selected.
    ctx.editorRef.current?.typeText(open);
  };
}
function symbolRun(char: string) {
  return (ctx: KeyActionContext): void => {
    // Typed, so the editor re-indents `else:` as it does for the keyboard's colon.
    ctx.editorRef.current?.typeText(char);
  };
}
function transformRun(make: (profile: LanguageProfile, unit: string) => (text: string, sel: Selection) => EditResult) {
  return (ctx: KeyActionContext): void => {
    ctx.editorRef.current?.transformDocument(make(profileOf(ctx.language), ctx.indentUnit));
  };
}
export const BUILTIN_KEYS: Record<BuiltinKeyId, BuiltinKeyDef> = {
  Tab: {
    name: 'Indent',
    icon: ArrowRightToLine,
    run: (ctx) => {
      Logger.info('toolbar.tab.run', {
        hasSelection: ctx.selection.start !== ctx.selection.end,
      });
      const indentNow = () => {
        if (ctx.selection.start !== ctx.selection.end) {
          ctx.editorRef.current?.transformDocument((text, sel) => indent(text, sel, ctx.indentUnit));
        } else {
          ctx.editorRef.current?.insertAtCursor(ctx.indentUnit);
        }
      };
      const editor = ctx.editorRef.current;
      if (!editor?.isCompletionOpen()) {
        indentNow();
        return;
      }
      // a suggestion is on show: the key takes it, and if the list went away in the meantime it indents as it always did
      void editor.acceptCompletion().then((accepted) => {
        if (!accepted) indentNow();
      });
    },
  },
  Dedent: {
    name: 'Dedent',
    icon: ArrowLeftToLine,
    run: transformRun((_profile, unit) => (text, sel) => dedent(text, sel, unit)),
  },
  Paren: {
    name: 'Parentheses',
    icon: Parentheses,
    run: pairRun('('),
  },
  Bracket: {
    name: 'Brackets',
    icon: Brackets,
    run: pairRun('['),
  },
  Brace: {
    name: 'Braces',
    icon: Braces,
    run: pairRun('{'),
  },
  Quote: {
    name: 'Double quote',
    label: '"',
    run: pairRun('"'),
  },
  Apostrophe: {
    name: 'Single quote',
    label: "'",
    run: pairRun("'"),
  },
  Colon: {
    name: 'Colon',
    label: ':',
    run: symbolRun(':'),
  },
  Equals: {
    name: 'Equals',
    label: '=',
    run: symbolRun('='),
  },
  Underscore: {
    name: 'Underscore',
    label: '_',
    run: symbolRun('_'),
  },
  Dot: {
    name: 'Dot',
    label: '.',
    run: symbolRun('.'),
  },
  Comment: {
    name: 'Toggle comment',
    icon: Hash,
    run: transformRun((profile) => (text, sel) => toggleComment(text, sel, profile)),
  },
  Newline: {
    name: 'New line (auto-indent)',
    icon: CornerDownLeft,
    run: transformRun((profile, unit) => (text, sel) => smartNewline(text, sel, profile, unit)),
  },
  Left: {
    name: 'Cursor left',
    icon: ArrowLeft,
    run: (ctx) => {
      Logger.info('toolbar.left.run', {});
      ctx.editorRef.current?.applySelection(moveLeft(ctx.text, ctx.selection));
    },
  },
  Right: {
    name: 'Cursor right',
    icon: ArrowRight,
    run: (ctx) => {
      Logger.info('toolbar.right.run', {});
      ctx.editorRef.current?.applySelection(moveRight(ctx.text, ctx.selection));
    },
  },
  Undo: {
    name: 'Undo',
    icon: RotateCcw,
    run: (ctx) => {
      Logger.info('toolbar.undo.run', {});
      ctx.editorRef.current?.undo();
    },
  },
  Redo: {
    name: 'Redo',
    icon: RotateCw,
    run: (ctx) => {
      Logger.info('toolbar.redo.run', {});
      ctx.editorRef.current?.redo();
    },
  },
};
