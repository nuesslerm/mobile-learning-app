/**
 * Fetches/posts comments on a golden-screenshot capture via the
 * `golden_screenshot_comments` ORDS auto-REST endpoint
 * (created once by the since-removed wf command `infra oracle db add-golden-screenshot-comments`).
 *
 * Reuses the same `EXPO_PUBLIC_HEALTH_REPORTS_ORDS_*` env vars as
 * `ordsClient.ts` rather than requiring a second vault-stored URL: the
 * comments table lives in the same ORDS-enabled schema as `health_reports`,
 * so its base URL is one sibling path segment away
 * (`.../health_reports/` -> `.../golden_screenshot_comments/`) — see
 * `deriveCommentsUrl`.
 */

import { Logger } from '@lib/logging';
import {
  goldenScreenshotCommentSchema,
  type GoldenScreenshotComment,
} from '../contracts';
import { z } from 'zod';

import { encodeBase64 } from '../lib/base64';
import { deriveCommentsUrl } from '../use-cases/deriveCommentsUrl';

const DEFAULT_ORDS_USER = 'health_rpt';

const ENV_BASE_URL_NAME = 'EXPO_PUBLIC_HEALTH_REPORTS_ORDS_BASE_URL';
const ENV_PASSWORD_NAME = 'EXPO_PUBLIC_HEALTH_REPORTS_ORDS_PASSWORD';

const ordsCommentRowSchema = z.object({
  id: z.number(),
  screenshot_key: z.string(),
  author: z.string().nullable().optional(),
  comment_text: z.string(),
  created_at: z.string(),
});

const ordsCommentsEnvelopeSchema = z.object({
  items: z.array(ordsCommentRowSchema),
});

export type GoldenScreenshotCommentsResult =
  | { status: 'not-configured'; reason: string }
  | { status: 'unreachable'; error: string }
  | { status: 'ready'; comments: GoldenScreenshotComment[] };

export type PostGoldenScreenshotCommentResult =
  | { status: 'not-configured'; reason: string }
  | { status: 'error'; error: string }
  | { status: 'ok' };

type OrdsConfig = { commentsUrl: string; user: string; password: string };

function loadConfig(): OrdsConfig | null {
  const healthReportsBaseUrl =
    process.env.EXPO_PUBLIC_HEALTH_REPORTS_ORDS_BASE_URL;
  const password = process.env.EXPO_PUBLIC_HEALTH_REPORTS_ORDS_PASSWORD;
  if (!healthReportsBaseUrl || !password) return null;
  const user =
    process.env.EXPO_PUBLIC_HEALTH_REPORTS_ORDS_USER || DEFAULT_ORDS_USER;
  return {
    commentsUrl: deriveCommentsUrl(healthReportsBaseUrl),
    user,
    password,
  };
}

function authHeader(config: OrdsConfig): string {
  return `Basic ${encodeBase64(`${config.user}:${config.password}`)}`;
}

function rowToComment(row: {
  id: number;
  screenshot_key: string;
  author?: string | null;
  comment_text: string;
  created_at: string;
}): GoldenScreenshotComment {
  return goldenScreenshotCommentSchema.parse({
    id: row.id,
    screenshotKey: row.screenshot_key,
    author: row.author ?? undefined,
    commentText: row.comment_text,
    createdAt: row.created_at,
  });
}

export async function fetchGoldenScreenshotComments(
  screenshotKey: string
): Promise<GoldenScreenshotCommentsResult> {
  const config = loadConfig();
  if (!config) {
    return {
      status: 'not-configured',
      reason: `${ENV_BASE_URL_NAME} / ${ENV_PASSWORD_NAME} are not set.`,
    };
  }

  const url = `${config.commentsUrl}?q=${encodeURIComponent(
    JSON.stringify({ screenshot_key: screenshotKey })
  )}`;

  let response: Response;
  try {
    response = await fetch(url, {
      method: 'GET',
      headers: { Authorization: authHeader(config) },
    });
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    Logger.warn('service_report.comments_fetch_failed', { err });
    return { status: 'unreachable', error: message };
  }

  if (!response.ok) {
    Logger.warn('service_report.comments_fetch_failed', {
      status: response.status,
    });
    return {
      status: 'unreachable',
      error: `ORDS request failed (HTTP ${response.status})`,
    };
  }

  let envelopeJson: unknown;
  try {
    envelopeJson = await response.json();
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    Logger.warn('service_report.comments_envelope_parse_failed', { err });
    return {
      status: 'unreachable',
      error: `Could not parse ORDS response: ${message}`,
    };
  }

  const envelope = ordsCommentsEnvelopeSchema.safeParse(envelopeJson);
  if (!envelope.success) {
    Logger.warn('service_report.comments_envelope_schema_invalid', {
      issues: envelope.error.issues,
    });
    return {
      status: 'unreachable',
      error:
        'ORDS response did not match the expected auto-REST envelope shape.',
    };
  }

  const comments = envelope.data.items
    .map(rowToComment)
    .sort((a, b) => Date.parse(a.createdAt) - Date.parse(b.createdAt));

  return { status: 'ready', comments };
}

export async function postGoldenScreenshotComment(input: {
  screenshotKey: string;
  author?: string;
  commentText: string;
}): Promise<PostGoldenScreenshotCommentResult> {
  const config = loadConfig();
  if (!config) {
    return {
      status: 'not-configured',
      reason: `${ENV_BASE_URL_NAME} / ${ENV_PASSWORD_NAME} are not set.`,
    };
  }

  // ORDS auto-REST binds every column omitted from the JSON body to NULL
  // rather than falling through to its own DEFAULT -- same quirk documented
  // in ordsHealthReports/client.ts's publishHealthReport -- so created_at is
  // always sent explicitly rather than relying on the column's SYSTIMESTAMP default.
  const body = {
    screenshot_key: input.screenshotKey,
    author: input.author,
    comment_text: input.commentText,
    created_at: new Date().toISOString(),
  };

  let response: Response;
  try {
    response = await fetch(config.commentsUrl, {
      method: 'POST',
      headers: {
        Authorization: authHeader(config),
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(body),
    });
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    Logger.warn('service_report.comment_post_failed', { err });
    return { status: 'error', error: message };
  }

  if (!response.ok) {
    Logger.warn('service_report.comment_post_failed', {
      status: response.status,
    });
    return {
      status: 'error',
      error: `ORDS request failed (HTTP ${response.status})`,
    };
  }

  return { status: 'ok' };
}
