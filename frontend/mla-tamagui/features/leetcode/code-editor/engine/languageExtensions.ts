import { cpp } from '@codemirror/lang-cpp';
import { javascript } from '@codemirror/lang-javascript';
import { python } from '@codemirror/lang-python';
import { Compartment, type Extension } from '@codemirror/state';
import type { Language } from '@lib/code-editor-api';

/** Holds the editor's language, so switching it reconfigures one compartment and leaves the text, the selection and the undo history alone. */
export const languageCompartment = new Compartment();

export function languageExtension(language: Language): Extension {
  switch (language) {
    case 'python':
      return python();
    case 'javascript':
      return javascript();
    case 'cpp':
      return cpp();
  }
}
