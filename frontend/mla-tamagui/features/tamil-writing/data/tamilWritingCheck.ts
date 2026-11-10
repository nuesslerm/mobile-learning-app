import { appConfig } from '@lib/app-config';
import { Logger } from '@lib/logging';

/**
 * Sends what was written to the agent for marking.
 *
 * The app cannot mark this itself. Measuring a traced line against a reference
 * works because the reference is right there; a letter written freehand at any
 * size, in any position, with any number of strokes, is a recognition problem,
 * and the honest way to answer it is to show the writing to something that can
 * read Tamil. That lives in `backend/remote-chat-agent`
 * (`POST /api/tamil/check`), which rasterises the SVG and puts it to a vision
 * model through the opencode harness.
 */

export type WritingVerdict = {
  ok: boolean;
  /** 0-100, the model's own confidence that this reads as the letter. */
  score: number;
  summary: string;
  fix?: string;
};

export type WritingCheckRequest = {
  characterId: string;
  glyph: string;
  transliteration: string;
  /** The ink alone, black on white -- never the template. */
  svg: string;
};

export class WritingCheckError extends Error {
  constructor(
    readonly status: number,
    message: string
  ) {
    super(message);
    this.name = 'WritingCheckError';
  }
}

const TIMEOUT_MS = 120_000;

export async function checkWriting(
  request: WritingCheckRequest
): Promise<WritingVerdict> {
  const baseUrl = appConfig.agentBaseUrl ?? 'http://localhost:8787';
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), TIMEOUT_MS);

  Logger.info('tamil.write.check', {
    characterId: request.characterId,
    svgBytes: request.svg.length,
  });

  let response: Response;
  try {
    response = await fetch(`${baseUrl}/api/tamil/check`, {
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
}
