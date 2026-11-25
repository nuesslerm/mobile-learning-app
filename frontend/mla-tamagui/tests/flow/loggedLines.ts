import '@/lib/logger';

import { drainAllBufferedSinks } from '@lib/logging/primitives';
import { afterEach, beforeEach, vi } from 'vitest';

const levels = ['debug', 'info', 'warn', 'error'] as const;
const observeConsole = () => levels.map((level) => ({ level, spy: vi.spyOn(console, level) }));
let observers: ReturnType<typeof observeConsole>;

beforeEach(() => {
  observers = observeConsole();
});
afterEach(() => {
  for (const { spy } of observers) spy.mockRestore();
});

// Error normalization may enqueue a task, even in a test using fake timers.
const realSetTimeout = globalThis.setTimeout;
async function drainLines() {
  await new Promise<void>((resolve) => realSetTimeout(resolve, 0));
  await drainAllBufferedSinks();
}

export async function forgetLoggedLines() {
  await drainLines();
  for (const { spy } of observers) spy.mockClear();
}

export async function loggedLines(event?: string) {
  await drainLines();
  const lines = observers.flatMap(({ level, spy }) =>
    spy.mock.calls.map(([message, fields], index) => ({
      order: spy.mock.invocationCallOrder[index]!,
      level,
      text: String(message),
      context: fields as Record<string, unknown> | undefined,
    }))
  );
  return lines.filter(({ text }) => event === undefined || text.includes(event)).sort((a, b) => a.order - b.order);
}
