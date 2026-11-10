// features/coding-agent/src/data/outboxStorage.ts
//
// AsyncStorage-backed persistence for the offline command queue (`OutboxItem[]`).
// The pure queue ops live in `src/domain/outbox.ts` so
// tests can reason about ordering and backoff without touching IO; this
// module is the thinnest possible IO shim — load/save round-trip through
// `OutboxItemSchema` so a corruption in storage (write half-finished by the
// OS, schema drift between app versions) is caught at the boundary instead
// of poisoning the reducer.
//
// The storage interface mirrors `CodingAgentStorage` from
// `codingAgentConfig.ts`: a `getItem`/`setItem` subset of AsyncStorage,
// injectable so tests can use an in-memory map without pulling in
// `@react-native-async-storage/async-storage`. The single key,
// `coding-agent.outbox`, is intentionally co-located with the other
// coding-agent keys (`coding-agent.token`) so the AsyncStorage namespace
// for the feature stays under one prefix.
import AsyncStorage from '@react-native-async-storage/async-storage';
import { Logger } from '@lib/logging';
import {
  OutboxItemSchema,
  type OutboxItem,
} from '../domain';

const CODING_AGENT_OUTBOX_STORAGE_KEY = 'coding-agent.outbox';

type OutboxStorage = {
  getItem(key: string): Promise<string | null>;
  setItem(key: string, value: string): Promise<void>;
};

export type OutboxStorageOptions = {
  storage?: OutboxStorage;
};

const defaultStorage: OutboxStorage = AsyncStorage;

function isOutboxItem(value: unknown): value is OutboxItem {
  return OutboxItemSchema.safeParse(value).success;
}

export async function loadOutbox(
  options: OutboxStorageOptions = {},
): Promise<OutboxItem[]> {
  const storage = options.storage ?? defaultStorage;
  let raw: string | null;
  try {
    raw = await storage.getItem(CODING_AGENT_OUTBOX_STORAGE_KEY);
  } catch (cause) {
    Logger.withError(cause).withMetadata({
      key: CODING_AGENT_OUTBOX_STORAGE_KEY,
    }).warn('coding-agent.outbox.load_failed');
    return [];
  }
  if (raw === null || raw.length === 0) return [];
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch (cause) {
    Logger.withError(cause).withMetadata({
      key: CODING_AGENT_OUTBOX_STORAGE_KEY,
    }).warn('coding-agent.outbox.parse_failed');
    return [];
  }
  if (!Array.isArray(parsed)) {
    Logger.withMetadata({
      key: CODING_AGENT_OUTBOX_STORAGE_KEY,
    }).warn('coding-agent.outbox.parse_failed');
    return [];
  }
  const items: OutboxItem[] = [];
  for (const entry of parsed) {
    if (isOutboxItem(entry)) {
      items.push(entry);
    }
  }
  warnDropped('load', parsed.length, items.length);
  return items;
}

function warnDropped(
  phase: 'load' | 'save',
  total: number,
  kept: number,
): void {
  if (kept === total) return;
  Logger.warn('coding-agent.outbox.items_dropped', {
    phase,
    dropped: total - kept,
    kept,
  });
}

export async function saveOutbox(
  items: readonly OutboxItem[],
  options: OutboxStorageOptions = {},
): Promise<void> {
  const storage = options.storage ?? defaultStorage;
  const validated: OutboxItem[] = [];
  for (const entry of items) {
    if (isOutboxItem(entry)) {
      validated.push(entry);
    }
  }
  warnDropped('save', items.length, validated.length);
  try {
    await storage.setItem(
      CODING_AGENT_OUTBOX_STORAGE_KEY,
      JSON.stringify(validated),
    );
  } catch (cause) {
    Logger.withError(cause).withMetadata({
      key: CODING_AGENT_OUTBOX_STORAGE_KEY,
    }).warn('coding-agent.outbox.save_failed');
  }
}
