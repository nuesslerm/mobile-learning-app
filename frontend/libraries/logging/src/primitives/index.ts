// Primitives — generic, no domain knowledge about log entries, the
// live tail, or the pipeline. Used by stages and transports.

export { createBufferedSink, drainAllBufferedSinks, flushAllBufferedSinks } from './bufferedSink.js';
export type { BufferedSink, BufferedSinkOptions } from './bufferedSink.js';

export { systemClock, createFrozenClock, createSteppingClock } from './clock.js';
export type { Clock } from './clock.js';

export { redactSensitiveFields, redactByKeyPrefix, scrubSecrets, createScrubber } from './secretRedaction.js';
export type { RedactRule, ScrubberOptions } from './secretRedaction.js';

export { allowedTraceparentOrigins, withTraceparent } from './traceparent.js';
export type { ActiveSpanLike, TraceparentFetchInit } from './traceparent.js';
