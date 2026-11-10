// Persistence layer — durable storage contract + in-memory fallback
// + Pipeline stage factory.
//
// Host supplies a `LogPersistence` adapter at boot:
//   - native: expo-sqlite
//   - web:     idb
//   - tests:   createInMemoryPersistence() (below)

export type {
  LogPersistence,
  PersistedLogEntry,
  StoreLine,
  QueryPageOptions,
} from './contract.js';
export { RETENTION_MAX_ROWS } from './contract.js';
export { createInMemoryPersistence } from './inMemoryPersistence.js';
export { appendToPersistence } from './appendToPersistence.js';
