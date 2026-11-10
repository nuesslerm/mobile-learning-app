import '@/lib/logger';

import { liveTail, type LogEntry } from '@lib/logging/liveTail';
import { drainAllBufferedSinks } from '@lib/logging/primitives';

// Captured now, while timers are real: a test under fake timers still gets its macrotask.
const realSetTimeout = globalThis.setTimeout;

// A line logged with an error attached reaches the tail one macrotask after the call.
const afterPendingLines = () => new Promise<void>((resolve) => realSetTimeout(resolve, 0));

/** Forgets every line logged so far, so a test reads only what its own steps log. */
export async function forgetLoggedLines(): Promise<void> {
  await afterPendingLines();
  await drainAllBufferedSinks();
  liveTail.clear();
}

/** The lines logged since `forgetLoggedLines`, oldest first; `event` keeps those whose message contains it. */
export async function loggedLines(event?: string): Promise<LogEntry[]> {
  await afterPendingLines();
  await drainAllBufferedSinks();
  const all = liveTail.getEntries();
  return event === undefined ? [...all] : all.filter((e) => e.text.includes(event));
}
