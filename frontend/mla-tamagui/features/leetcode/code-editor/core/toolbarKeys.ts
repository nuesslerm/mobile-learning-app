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

import type { BuiltinKeyId } from '../../domain';
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
  Pencil,
  RotateCcw,
  RotateCw,
} from '@lib/tamagui-design-system/icons';
import type { RefObject } from 'react';

import type { EditorInputHandle } from './EditorInput';
import {
  INDENT_UNIT,
  dedent,
  getWordAtSelection,
  indent,
  moveLeft,
  moveRight,
  smartNewline,
  toggleComment,
  wrapSelection,
  type EditResult,
} from './editing';
import type { Selection } from './types';
export type KeyActionContext = {
  editorRef: RefObject<EditorInputHandle | null>;
  text: string;
  selection: Selection;
  onRenameSymbol?: (symbolName: string) => void;
};
export type BuiltinKeyDef = {
  /** Human-readable name shown on the configure screen. */
  name: string;
  /** Either an icon or a glyph label is shown on the key (never both). */
  icon?: LucideIcon;
  label?: string;
  run: (ctx: KeyActionContext) => void;
};
function applyEdit(ctx: KeyActionContext, result: EditResult): void {
  ctx.editorRef.current?.setTextAndSelection(result.text, result.selection);
}
function pairRun(open: string, close: string) {
  return (ctx: KeyActionContext): void => {
    if (ctx.selection.start !== ctx.selection.end) {
      applyEdit(ctx, wrapSelection(ctx.text, ctx.selection, open, close));
    } else {
      // Typed, not inserted: a plain insert of the pair left typing the closer to add a second one.
      ctx.editorRef.current?.typeText(open);
    }
  };
}
function symbolRun(char: string) {
  return (ctx: KeyActionContext): void => {
    // Typed, so the editor re-indents `else:` as it does for the keyboard's colon.
    ctx.editorRef.current?.typeText(char);
  };
}
function transformRun(fn: (text: string, sel: Selection) => EditResult) {
  return (ctx: KeyActionContext): void => {
    applyEdit(ctx, fn(ctx.text, ctx.selection));
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
      if (ctx.selection.start !== ctx.selection.end) {
        applyEdit(ctx, indent(ctx.text, ctx.selection));
      } else {
        ctx.editorRef.current?.insertAtCursor(INDENT_UNIT);
      }
    },
  },
  Dedent: {
    name: 'Dedent',
    icon: ArrowLeftToLine,
    run: transformRun(dedent),
  },
  Paren: {
    name: 'Parentheses',
    icon: Parentheses,
    run: pairRun('(', ')'),
  },
  Bracket: {
    name: 'Brackets',
    icon: Brackets,
    run: pairRun('[', ']'),
  },
  Brace: {
    name: 'Braces',
    icon: Braces,
    run: pairRun('{', '}'),
  },
  Quote: {
    name: 'Double quote',
    label: '"',
    run: pairRun('"', '"'),
  },
  Apostrophe: {
    name: 'Single quote',
    label: "'",
    run: pairRun("'", "'"),
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
    run: transformRun(toggleComment),
  },
  Newline: {
    name: 'New line (auto-indent)',
    icon: CornerDownLeft,
    run: transformRun(smartNewline),
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
  RenameSymbol: {
    name: 'Rename symbol',
    icon: Pencil,
    run: (ctx) => {
      const word = getWordAtSelection(ctx.text, ctx.selection);
      Logger.info('toolbar.rename.run', {
        hasWord: Boolean(word),
      });
      if (word) ctx.onRenameSymbol?.(word);
    },
  },
};
