// features/coding-agent/src/domain/outbox.ts
//
// Pure queue for offline-first command delivery to the coding-agent WS.
// Ported in spirit from references/t3code/apps/mobile/src/state/thread-outbox.ts
// (no `effect`, no IO). Each item carries the command payload, the thread it
// belongs to, when it was queued, and how many delivery attempts have run.
// `nextDue` picks the next item the drain can ship, honoring the per-item
// linear backoff (`attempts × 2s`) so a retry waits before re-firing. The
// actual drain lives in the impl package; this module only owns the data
// shape and the pure ops.

import { ThreadId } from '@app/remote-chat-agent-contracts/orchestration';
import { z } from 'zod';

export const OutboxItemSchema = z.object({
  id: z.string().min(1),
  threadId: ThreadId,
  command: z.unknown(),
  createdAt: z.number().nonnegative(),
  attempts: z.number().int().nonnegative(),
});

export type OutboxItem = z.infer<typeof OutboxItemSchema>;

export function enqueue(
  items: ReadonlyArray<OutboxItem>,
  item: OutboxItem,
): OutboxItem[] {
  return [...items, item];
}

export function markAttempt(
  items: ReadonlyArray<OutboxItem>,
  id: string,
): OutboxItem[] {
  return items.map((item) =>
    item.id === id ? { ...item, attempts: item.attempts + 1 } : item,
  );
}

export function remove(
  items: ReadonlyArray<OutboxItem>,
  id: string,
): OutboxItem[] {
  return items.filter((item) => item.id !== id);
}

function defaultBackoffMs(attempts: number): number {
  return attempts * 2_000;
}

/**
 * Pick the next outbox item the drain can ship: the one whose
 * `createdAt + backoffMs(attempts)` is the earliest in the past relative to
 * `now`. Items still inside their backoff window are skipped. Returns
 * `undefined` when no item is due.
 */
export function nextDue(
  items: ReadonlyArray<OutboxItem>,
  now: number,
  backoffMs: (attempts: number) => number = defaultBackoffMs,
): OutboxItem | undefined {
  let best: OutboxItem | undefined;
  let bestDueAt = Number.POSITIVE_INFINITY;
  for (const item of items) {
    const dueAt = item.createdAt + backoffMs(item.attempts);
    if (dueAt > now) {
      continue;
    }
    if (dueAt < bestDueAt) {
      best = item;
      bestDueAt = dueAt;
    }
  }
  return best;
}
