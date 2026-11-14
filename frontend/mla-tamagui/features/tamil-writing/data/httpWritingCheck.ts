import { Logger } from '@lib/logging';

import {
  WritingCheckError,
  type WritingCheckPort,
  type WritingCheckRequest,
  type WritingVerdict,
} from '../domain/ports';

/**
 * Sends what was written to the agent for marking: `POST /api/tamil/check` in `backend/remote-chat-agent`, which
 * rasterises the SVG and puts it to a vision model through the opencode harness.
 */
const TIMEOUT_MS = 120_000;

export function createHttpWritingCheck(baseUrl: string, fetchImpl: typeof fetch = fetch): WritingCheckPort {
  return {
    async check(request: WritingCheckRequest): Promise<WritingVerdict> {
      const controller = new AbortController();
      const timer = setTimeout(() => controller.abort(), TIMEOUT_MS);

      Logger.info('tamil.write.check', {
        characterId: request.characterId,
        svgBytes: request.svg.length,
      });

      let response: Response;
      try {
        response = await fetchImpl(`${baseUrl}/api/tamil/check`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(request),
          signal: controller.signal,
        });
      } catch (error) {
        const message =
          (error instanceof Error ? error.name : '') === 'AbortError'
            ? `No answer after ${TIMEOUT_MS / 1000}s`
            : error instanceof Error
              ? error.message
              : String(error);
        throw new WritingCheckError(0, message);
      } finally {
        clearTimeout(timer);
      }

      if (!response.ok) {
        // A gateway's HTML error page would otherwise ride along in the log line whole.
        throw new WritingCheckError(response.status, (await response.text()).slice(0, 200));
      }
      return (await response.json()) as WritingVerdict;
    },
  };
}
