// Stage contract — the building block of `PipelineTransport`.
//
// A stage is a function that processes one batch of entries produced by
// the pipeline. Stages can be sync or async. The pipeline fires each
// stage with the same batch reference; later stages see the same
// reference, so stages MUST be read-only on the batch (mutating would
// corrupt later stages).
//
//   pipelineStages = [
//     appendToLiveTail(),
//     appendToPersistence(persistence),
//   ];
//
// Conventions:
//   - Stages are READ-ONLY on the batch.
//   - Stages must not throw (the pipeline wraps each call in try/catch).
//   - Stages can be sync or async; async results are awaited but
//     rejections are swallowed (logging must never crash).
//   - Side effects are the whole point — the stage does the work of
//     forwarding the batch to wherever it needs to go.

export type Stage<T> = (batch: readonly T[]) => void | Promise<void>;
