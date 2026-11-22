/** A language an on-device runner can run: the name its page and its warm-up are keyed by. */
export type DeviceLanguage = 'python3' | 'javascript';

export const isPythonLanguage = (lang: string): boolean => lang === 'python3' || lang === 'python';

export const isJavaScriptLanguage = (lang: string): boolean => lang === 'javascript';

/** The on-device language an editor language runs as, or null when no on-device runner exists for it. */
export function deviceLanguageOf(lang: string): DeviceLanguage | null {
  if (isPythonLanguage(lang)) return 'python3';
  if (isJavaScriptLanguage(lang)) return 'javascript';
  return null;
}
