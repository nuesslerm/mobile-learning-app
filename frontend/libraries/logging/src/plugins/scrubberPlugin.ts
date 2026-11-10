// Secret + PII scrubbing as a LogLayer plugin — the automatic, run-once-before-
// every-transport insertion point. Unlike the read-only `redactSensitiveFields`
// pipeline Stage (a no-op under the batch contract), a plugin's `onBeforeDataOut`
// / `onBeforeMessageOut` hooks transform the metadata and message BEFORE they
// reach any transport (console echo, persistence, remote), so nothing sensitive
// is ever stored or sent — not even for one frame.
//
// Wired in by default via `configureLogger` (opt out with `configureLogger({
// scrub: false })`). Covers: sensitive field-name keys, the values of
// sensitive-named env vars wherever they appear, and PII patterns. See
// `createScrubber` for the redaction rules.

import type { LogLayerPlugin } from 'loglayer';

import {
  createScrubber,
  type ScrubberOptions,
} from '../primitives/secretRedaction.js';

export function scrubberPlugin(opts?: ScrubberOptions): LogLayerPlugin {
  const scrub = createScrubber(opts);
  return {
    id: 'secret-pii-scrubber',
    onBeforeDataOut: ({ data }) => (data ? scrub(data) : data),
    onBeforeMessageOut: ({ messages }) =>
      messages.map((m) => (typeof m === 'string' ? scrub(m) : m)),
  };
}
