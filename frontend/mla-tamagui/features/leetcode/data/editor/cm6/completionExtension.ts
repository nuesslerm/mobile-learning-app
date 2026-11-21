import { acceptCompletion, autocompletion, closeCompletion, currentCompletions } from '@codemirror/autocomplete';
import { Prec, type Extension } from '@codemirror/state';
import { EditorView, keymap } from '@codemirror/view';
import type { CompletionEntry, Language } from '@lib/code-editor-api';

import { createCompletionSource } from './completionSources';

export type CompletionExtensionOptions = {
  readonly language: Language;
  readonly enabled: boolean;
  readonly entries: readonly CompletionEntry[];
  /** told once each time the list opens or closes */
  readonly onOpenChange: (open: boolean) => void;
};

const ROW_PX = 40;
// a finger, not a pointer: rows of a touch size, three and a half of them so the list scrolls, and the popup never wider than the screen
const phoneTheme = EditorView.baseTheme({
  '.cm-tooltip.cm-tooltip-autocomplete > ul': { maxHeight: `${ROW_PX * 3.5}px`, minWidth: 'min(240px, 80vw)', fontFamily: 'inherit' },
  '.cm-tooltip.cm-tooltip-autocomplete > ul > li': { minHeight: `${ROW_PX}px`, lineHeight: `${ROW_PX}px`, padding: '0 12px', display: 'flex', alignItems: 'center' },
});

// a keyboard that hides for a moment must not take the list with it: the popup closes only if the editor still has no focus a moment later
const BLUR_GRACE_MS = 150;

/**
 * The editor's completion, shaped for a phone: nothing in the state while it is off; while on, the problem's own names first, a list above
 * the caret, Tab (the toolbar's Indent key) to accept and Enter never to accept, so a person who types a name and presses Return gets a
 * new line and not a surprise.
 */
export function completionExtensions({ language, enabled, entries, onOpenChange }: CompletionExtensionOptions): Extension {
  if (!enabled) return [];
  let open = false;
  return [
    autocompletion({
      override: [createCompletionSource({ language, entries })],
      defaultKeymap: false,
      icons: false,
      filterStrict: true,
      closeOnBlur: false,
      interactionDelay: 0,
      aboveCursor: true,
      maxRenderedOptions: 12,
    }),
    phoneTheme,
    Prec.high(
      keymap.of([
        { key: 'Tab', run: acceptCompletion },
        { key: 'Escape', run: closeCompletion },
      ])
    ),
    EditorView.domEventHandlers({
      blur(_event, view) {
        setTimeout(() => {
          if (!view.hasFocus) closeCompletion(view);
        }, BLUR_GRACE_MS);
        return false;
      },
    }),
    EditorView.updateListener.of((update) => {
      const now = currentCompletions(update.state).length > 0;
      if (now === open) return;
      open = now;
      onOpenChange(now);
    }),
  ];
}
