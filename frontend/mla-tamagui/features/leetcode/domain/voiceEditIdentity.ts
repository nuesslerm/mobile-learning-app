/**
 * The two things every voice or plan edit needs before it has a screen: the
 * id that joins a turn's client row to its server rows, and the one-line form
 * of a planner failure. They live here rather than in the editor screen so a
 * hook can use them without importing the UI it renders inside -- an import
 * that pulled the whole screen (and its toast, router and bridge) into every
 * hook test.
 */

/** Hermes has no crypto.randomUUID; this only has to be unique per device per session. */
export function newEditId(): string {
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

/**
 * A planner failure arrives as the server's whole JSON envelope, raw LLM
 * output included. The banner shows the first line, shortened; the full text
 * is in the `voice_edit.plan.error` log line.
 */
export function compactVoiceError(message: string | null): string {
  if (!message) return 'Voice edit failed.';
  const firstLine = message.split('\n')[0].replace(/\s+/g, ' ').trim();
  const cut = firstLine.indexOf('{');
  const head = cut > 0 ? firstLine.slice(0, cut).trim() : firstLine;
  return head.length > 140 ? `${head.slice(0, 137)}…` : head;
}

/** A transport failure's HTTP status (0: no response); the logger's Error serialiser drops it. */
export function errorStatus(err: unknown): number {
  const status = (err as { status?: unknown } | null)?.status;
  return typeof status === 'number' ? status : 0;
}
