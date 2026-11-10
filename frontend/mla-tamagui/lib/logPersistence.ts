import type {
  LogPersistence,
  PersistedLogEntry,
  QueryPageOptions,
} from '@lib/logging/persistence';
import { createInMemoryPersistence } from '@lib/logging/persistence';

let logPersistence: LogPersistence | null = null;

function getPersistence(): LogPersistence {
  if (!logPersistence) {
    logPersistence = createInMemoryPersistence();
  }
  return logPersistence;
}

export function setPersistence(persistence: LogPersistence): void {
  logPersistence = persistence;
}

export async function queryPage(opts: QueryPageOptions): Promise<
  PersistedLogEntry[]
> {
  return getPersistence().queryPage(opts);
}

export async function clear(): Promise<void> {
  return getPersistence().clear();
}
