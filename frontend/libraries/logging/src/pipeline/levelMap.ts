// Maps LogLayer's level vocabulary onto the store's `LogLevel` so downstream
// consumers (overlay UI, persistence backend, remote forwarder) see one
// stable level enum.
//
// LogLayer exposes the standard 6 (`trace`/`debug`/`info`/`warn`/`error`/
// `fatal`). The store only knows 5 (`debug`/`log`/`info`/`warn`/`error`)
// because the overlay doesn't render a separate `fatal` badge — fatal is
// rendered as error. `log` is the legacy alias used by raw console
// captures before `console.log` got re-mapped.

import type { LogLevel } from '../liveTail/index.js';

export type LogLayerLevel =
  | 'trace'
  | 'debug'
  | 'info'
  | 'warn'
  | 'error'
  | 'fatal';

/**
 * Map a LogLayer level to the store's level vocabulary.
 * - `trace` → `debug` (lowest visible severity)
 * - `fatal` → `error` (highest severity, same UI badge)
 * - everything else passes through
 */
export function toStoreLevel(level: LogLayerLevel): LogLevel {
  switch (level) {
    case 'trace':
      return 'debug';
    case 'fatal':
      return 'error';
    case 'debug':
    case 'info':
    case 'warn':
    case 'error':
      return level;
  }
}
