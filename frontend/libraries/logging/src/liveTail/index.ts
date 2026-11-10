// In-memory log tail — the snapshot-based pub-sub buffer that drives
// the on-screen overlay UI. Co-locates the liveTail singleton with
// its `appendToLiveTail` Pipeline stage so consumers import from one place.

export {
  liveTail,
  safeJson,
  MAX_LIVE_ENTRIES,
  __resetLiveTail,
  __setLiveTailClock,
} from './liveTail.js';
export type {
  LogEntry,
  LogLevel,
  AddOptions,
  LiveTailListener,
} from './liveTail.js';
export { appendToLiveTail } from './appendToLiveTail.js';
