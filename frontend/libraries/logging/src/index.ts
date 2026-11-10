// Public API for `@lib/logging`.
//
// Layered architecture (top → bottom, callers read top-down):
//   logger/      — JS-facing API (Logger)
//   babel/       — compile-time call-site injection (injectLoggerSource)
//   plugins/     — the scrubber loglayer plugin
//   primitives/  — generic, no domain knowledge (bufferedSink, clock, secretRedaction, traceparent)
//   liveTail/    — in-memory log tail + appendToLiveTail Pipeline stage
//   persistence/ — durable storage contract + impls + appendToPersistence stage
//   pipeline/    — composition (PipelineTransport, built-in ConsoleEchoTransport, levelMap)
//   install/     — generic platform hookups (installConsoleCapture)

export {
  Logger,
  configureLogger,
  isLoggerConfigured,
  __resetLoggerConfig,
} from './logger/Logger.js';

export { installConsoleCapture, restoreConsole, getOriginalConsole } from './install/installConsoleCapture.js';
export type { InstallConsoleCaptureOptions, ConsoleCaptureTarget } from './install/installConsoleCapture.js';

export type { LogEntry, LogLevel, AddOptions, LiveTailListener } from './liveTail/index.js';
export type { LogPersistence, PersistedLogEntry, StoreLine, QueryPageOptions } from './persistence/index.js';

export type {
  LoggerConfig,
  LogChain,
  WideEventFields,
  ErrorSerializer,
} from './types.js';
