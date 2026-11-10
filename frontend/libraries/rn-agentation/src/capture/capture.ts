import type { AgentationLogger, CaptureResult } from '../domain/types';

export interface CaptureOptions {
  logger?: AgentationLogger;
  /**
   * Native only: the app-content root host instance to scope the hit-test to
   * (so the capture overlay itself is never resolved). Ignored on web.
   */
  inspectedView?: unknown;
}

/**
 * Default capture stub. Metro resolves `capture.web.ts` (Expo web) or
 * `capture.native.ts` (Expo Go / native) ahead of this file; this exists so the
 * import type-checks and degrades gracefully in any other environment.
 */
export function captureAtPoint(
  _locationX: number,
  _locationY: number,
  _options?: CaptureOptions,
): Promise<CaptureResult> {
  return Promise.resolve({ componentName: 'unknown' });
}
