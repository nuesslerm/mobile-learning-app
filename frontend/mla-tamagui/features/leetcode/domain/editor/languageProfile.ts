import type { Language } from '@lib/code-editor-api';

/** What `editing.ts` needs to know about a language to edit it the way its grammar does. */
export type LanguageProfile = {
  /** The line comment marker. */
  readonly lineComment: string;
  /** A line that ends with one of these opens a block: the next line goes one level in. */
  readonly openers: readonly string[];
  /** A line that matches this opens a block without ending in an opener: `if (x)` with no brace after it. */
  readonly statementOpener?: RegExp;
  /** An opener directly followed by its closer, with the caret between: Enter puts the closer on its own line. */
  readonly pairs: readonly string[];
};

export const PYTHON_PROFILE: LanguageProfile = { lineComment: '#', openers: [':'], pairs: [] };

// the grammar's own Enter indents after a braceless if, while and for, and not after else, case or default
const BRACELESS_CONTROL = /^\s*(?:\}\s*)?(?:else\s+)?(?:if|while|for)\b.*\)$/;

const JAVASCRIPT_PROFILE: LanguageProfile = {
  lineComment: '//',
  openers: ['{', '(', '[', '=>'],
  statementOpener: BRACELESS_CONTROL,
  pairs: ['{}', '()', '[]'],
};

const CPP_PROFILE: LanguageProfile = {
  lineComment: '//',
  openers: ['{', '('],
  statementOpener: BRACELESS_CONTROL,
  // not "[]": the grammar's Enter splits a C++ subscript too, but without the extra indent, which a profile cannot tell from a bracket
  // that wants it, so the caret between them gets a plain line break
  pairs: ['{}', '()'],
};

export const LANGUAGE_PROFILES: Record<Language, LanguageProfile> = {
  python: PYTHON_PROFILE,
  javascript: JAVASCRIPT_PROFILE,
  cpp: CPP_PROFILE,
};

export const profileOf = (language: Language): LanguageProfile => LANGUAGE_PROFILES[language];

/** The editor language of the screen's language id, or null for one the editor has no grammar for. */
export function toEditorLanguage(id: string): Language | null {
  switch (id) {
    case 'python':
    case 'python3':
      return 'python';
    case 'javascript':
      return 'javascript';
    case 'cpp':
      return 'cpp';
    default:
      return null;
  }
}
