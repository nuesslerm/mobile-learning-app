// Pipeline layer — composition of buffering + per-batch stages.
//
// Note: the stage factories (appendToLiveTail, appendToPersistence) live in
// their domain folders (liveTail/, persistence/) — re-exported here as a
// convenience for hosts wiring the pipeline.

export { PipelineTransport } from './pipelineTransport.js';
export type { PipelineTransportOptions } from './pipelineTransport.js';
export type { Stage } from './stage.js';
export { toStoreLevel } from './levelMap.js';
export type { LogLayerLevel } from './levelMap.js';
export { ConsoleEchoTransport } from './builtInTransports/consoleTransport.js';
export type {
  ConsoleEchoTransportOptions,
  ConsoleSink,
} from './builtInTransports/consoleTransport.js';

// Re-export the distributed stage factories so hosts can pull
// everything they need from one place.
export { appendToLiveTail } from '../liveTail/appendToLiveTail.js';
export { appendToPersistence } from '../persistence/appendToPersistence.js';

export { appendToBetterStack } from './appendToBetterStack.js';
export type { AppendToBetterStackOptions, BetterStackFetch } from './appendToBetterStack.js';
