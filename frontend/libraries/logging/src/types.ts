// Public type surface for `@lib/logging`.
//
// Transport-agnostic — the host app supplies the concrete `LogLayer` instance
// via `configureLogger()` at boot, and all subsequent log calls route through
// it. This means the package can be used by the mobile app, Storybook, unit
// tests, and any future host (Storybook web, server-side scripts, etc.) by
// passing a different transport config.

import type { ILogBuilder, ILogLayer } from 'loglayer';
import type { ScrubberOptions } from './primitives/secretRedaction.js';

/** A bag of structured fields attached to a log call. */
export type WideEventFields = Record<string, unknown>;

/** Anything that accepts a level method (info / warn / etc.). */
export type LogChain = ILogLayer | ILogBuilder;

/**
 * Wiring contract supplied by the host app via `configureLogger()`.
 *
 * `root` is a getter (not a value) so the host can rebuild the root layer at
 * runtime — e.g. on dev restart — without re-configuring this package.
 */
export type LoggerConfig = {
  root: () => ILogLayer | null;
  /**
   * Secret + PII scrubbing, ON by default. `configureLogger` attaches the
   * scrubber plugin to the root layer so every log (all transports) is scrubbed
   * before it leaves. Pass `false` to opt out, or a `ScrubberOptions` object to
   * tune it (custom key patterns, disable PII, extra literal values, …).
   */
  scrub?: boolean | ScrubberOptions;
};

/**
 * Error serializer passed to `new LogLayer({ errorSerializer })`.
 *
 * Plain `JSON.stringify` on an Error yields `{}`, so the host typically
 * extracts `name`, `message`, `stack`, `code`, `cause`. The default in this
 * package is a no-op — host apps should override via their `LogLayer`
 * constructor (this package doesn't own the root instance).
 */
export type ErrorSerializer = (err: unknown) => Record<string, unknown>;
