import { appConfig } from '@lib/app-config';
import { Logger } from '@lib/logging';
import {
  allowedTraceparentOrigins,
  withTraceparent,
} from '@lib/logging/primitives';
import type { ClientEditStat, EditSource, VoiceEditResponse } from '@lib/voice-edit-dsl';
import { Platform } from 'react-native';

import type { VoiceEditTransportPort } from '../domain/ports';

import { errorStatus } from './voiceEditIdentity';

export type VoiceTranscribeRequest = {
  audioUri: string;
  mimeType: string;
  filename: string;
  /** Zero-based position of this clip inside a live-transcript session. */
  chunkIndex?: number;
};

export type { ClientEditStat };

type VoicePlanRequest = {
  transcript: string;
  currentCode: string;
  language: string;
  /** How the instruction was produced. The server stores it with the row; nothing else reads it. */
  source?: EditSource;
  /** One id per edit turn, shared by the transcribe call, the plan call and the stats row. */
  editId?: string;
};

/** Wall time of the whole HTTP call as the client saw it -- request sent to body parsed. */
export type Timed<T> = T & { wallMs: number };

export class VoiceEditTransportError extends Error {
  constructor(
    public readonly status: number,
    message: string
  ) {
    super(message);
    this.name = 'VoiceEditTransportError';
  }
}

// Whisper transcription + the LLM planning step routinely take 20-70s in
// practice, so this has to be generous -- but a plain `fetch()` with no
// signal hangs FOREVER if the backend or the LLM call ever stalls, since
// nothing ever rejects the promise to reach the caller's catch block. Found
// live: a real run sat with no response and no error for over a minute
// before the check gave up, indistinguishable from "the app did nothing".
const VOICE_EDIT_TIMEOUT_MS = 180_000;

/**
 * Appends a local recording to FormData the way each platform's networking
 * layer actually supports it.
 *
 * On native (iOS/Android), `fetch(uri).then(r => r.blob())` followed by
 * `form.append('audio', blob, filename)` is a well-known RN footgun: RN's
 * Blob is a JS-side polyfill backed by a native blob manager that resolves
 * the real bytes only when the request is actually sent, and that
 * resolution has repeatedly truncated/corrupted binary (non-text) payloads
 * under Hermes -- which reads on the server as exactly "not a valid file"
 * (e.g. ffmpeg's "moov atom not found"), regardless of how long the source
 * recording actually was. RN's `fetch`/`FormData` special-cases a plain
 * `{uri, name, type}` object instead, reading the file directly through
 * native code with no blob indirection -- the documented, reliable way to
 * upload a local file there.
 *
 * On web, the opposite is true: `FormData` there is the real browser API,
 * which requires its second argument to actually be a `Blob`/`File` --
 * confirmed live, passing the RN-shaped `{uri, name, type}` object throws
 * `Failed to execute 'append' on 'FormData': parameter 2 is not of type
 * 'Blob'` immediately, before any network call happens. Web's own Blob is a
 * native browser primitive (not RN's polyfill), so there's no truncation
 * risk here -- the plain blob is exactly right.
 */
async function appendAudioFile(
  form: FormData,
  audioUri: string,
  mimeType: string,
  filename: string
): Promise<void> {
  if (Platform.OS !== 'web') {
    // Reading the clip into a Blob only to log its size makes expo/fetch warn
    // ("Response.blob() is using React Native's ...") on every upload.
    Logger.withMetadata({ audioUri, mimeType, filename }).info(
      'voice_edit.audio_file'
    );
    form.append(
      'audio',
      { uri: audioUri, name: filename, type: mimeType } as unknown as Blob,
      filename
    );
    return;
  }

  let byteSize: number | 'unknown' = 'unknown';
  let blob: Blob | undefined;
  try {
    const probe = await fetch(audioUri);
    blob = await probe.blob();
    byteSize = blob.size;
  } catch (err) {
    Logger.withError(err).warn('voice_edit.size_probe_failed');
  }
  Logger.withMetadata({ audioUri, mimeType, filename, byteSize }).info(
    'voice_edit.audio_file'
  );

  if (!blob) {
    throw new Error(`Could not read recording at ${audioUri} for upload`);
  }
  form.append('audio', blob, filename);
}

/**
 * Uploads a FormData on a phone through XMLHttpRequest. SDK 57's global `fetch` is expo/fetch, which rejects RN's
 * `{ uri, name, type }` file part ("Unsupported FormDataPart implementation", seen on Android on redroid, 2026-10-11)
 * and RN's Blob alike; XHR still reads the file natively from that part.
 */
function postFormXhr<T>(url: string, form: FormData, timeoutMs: number): Promise<Timed<T>> {
  const sentAt = Date.now();
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    xhr.open('POST', url);
    xhr.timeout = timeoutMs;
    xhr.onload = () => {
      if (xhr.status < 200 || xhr.status >= 300) {
        reject(new VoiceEditTransportError(xhr.status, String(xhr.responseText)));
        return;
      }
      try {
        resolve({ ...(JSON.parse(String(xhr.responseText)) as T), wallMs: Date.now() - sentAt });
      } catch (err) {
        Logger.withError(err).warn('voice_edit.xhr_parse_failed');
        reject(new VoiceEditTransportError(0, err instanceof Error ? err.message : String(err)));
      }
    };
    xhr.onerror = () => reject(new VoiceEditTransportError(0, 'Network request failed'));
    xhr.ontimeout = () => reject(new VoiceEditTransportError(0, `No response after ${timeoutMs / 1000}s`));
    xhr.send(form);
  });
}

/**
 * Shared POST + timeout + error-normalization path for every voice-edit-
 * pipeline call. `AbortSignal.timeout()` isn't implemented in Hermes (React
 * Native's JS engine) despite working fine in every browser-based check this
 * was verified against -- found live on a real device: "AbortSignal.timeout
 * is not a function". A plain AbortController + setTimeout is the
 * universally-supported equivalent. A plain `fetch()` with no signal at all
 * hangs FOREVER if the backend or an LLM call ever stalls, since nothing
 * ever rejects the promise to reach the caller's catch block -- found live:
 * a real run sat with no response and no error for over a minute before the
 * check gave up, indistinguishable from "the app did nothing".
 */
async function postJson<T>(
  url: string,
  body: BodyInit,
  headers?: Record<string, string>,
  timeoutMs = VOICE_EDIT_TIMEOUT_MS
): Promise<Timed<T>> {
  if (Platform.OS !== 'web' && typeof body !== 'string') {
    return postFormXhr<T>(url, body as unknown as FormData, timeoutMs);
  }
  const sentAt = Date.now();
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  let res: Response;
  try {
    res = await fetch(
      url,
      withTraceparent(
        url,
        { method: 'POST', body, headers, signal: controller.signal },
        null,
        allowedTraceparentOrigins([
          appConfig.agentBaseUrl,
          appConfig.leetcodeBackendBaseUrl,
        ])
      )
    );
  } catch (err) {
    // An aborted fetch rejects with an `AbortError` (DOMException on web,
    // a plain Error with that name on Hermes) -- normalize both that and a
    // network-level failure (connection refused, CORS rejection) into the
    // same transport error the caller already knows how to surface as
    // phase 'error'.
    const name = err instanceof Error ? err.name : undefined;
    const message =
      name === 'AbortError'
        ? `No response after ${timeoutMs / 1000}s`
        : err instanceof Error
          ? err.message
          : String(err);
    throw new VoiceEditTransportError(0, message);
  } finally {
    clearTimeout(timer);
  }

  if (!res.ok) {
    const text = await res.text();
    throw new VoiceEditTransportError(res.status, text);
  }

  const data = (await res.json()) as T;
  return { ...data, wallMs: Date.now() - sentAt };
}

export type VoiceTranscribeResponse = { transcript: string };

/**
 * Transcribes a recording WITHOUT engaging the LLM planning step -- lets the
 * caller show the transcript for review/editing before committing to an
 * edit. Same upload path as sendVoiceEdit, just a different backend route.
 */
export async function sendVoiceTranscribe({
  audioUri,
  mimeType,
  filename,
  editId,
  chunkIndex,
}: VoiceTranscribeRequest & { editId?: string }): Promise<Timed<VoiceTranscribeResponse>> {
  const baseUrl = appConfig.agentBaseUrl ?? 'http://localhost:8787';
  const form = new FormData();
  await appendAudioFile(form, audioUri, mimeType, filename);
  if (editId) form.append('editId', editId);
  if (chunkIndex !== undefined) form.append('chunkIndex', String(chunkIndex));

  return postJson<VoiceTranscribeResponse>(
    `${baseUrl}/api/voice-transcribe`,
    form as unknown as BodyInit
  );
}

/**
 * Plans code-edit alternatives from an already-known transcript (the user's
 * own edited text, or the one whisper produced) -- no audio involved.
 */
async function sendVoicePlan({
  transcript,
  currentCode,
  language,
  source,
  editId,
}: VoicePlanRequest): Promise<Timed<VoiceEditResponse>> {
  const baseUrl = appConfig.agentBaseUrl ?? 'http://localhost:8787';

  return postJson<VoiceEditResponse>(
    `${baseUrl}/api/voice-plan`,
    JSON.stringify({ transcript, currentCode, language, source, editId }),
    { 'Content-Type': 'application/json' }
  );
}

const EDIT_STATS_TIMEOUT_MS = 8_000;

/**
 * Best-effort: a stats row must never cost the user a turn. Failures are
 * logged once and swallowed; the caller does not await this.
 */
export async function sendEditStats(row: ClientEditStat): Promise<void> {
  const baseUrl = appConfig.agentBaseUrl ?? 'http://localhost:8787';
  // One retry: a single dropped request on the tailnet must not lose the row (seen on Android, status 0).
  for (let attempt = 1; attempt <= 2; attempt += 1) {
    try {
      await postJson<{ ok: boolean }>(
        `${baseUrl}/api/edit-stats`,
        JSON.stringify(row),
        { 'Content-Type': 'application/json' },
        EDIT_STATS_TIMEOUT_MS
      );
      Logger.info('edit_stats.sent', { editId: row.editId, outcome: row.outcome, attempt });
      return;
    } catch (err) {
      if (attempt === 2) {
        Logger.withError(err).warn('edit_stats.send_failed', {
          editId: row.editId,
          outcome: row.outcome,
          status: errorStatus(err),
        });
        return;
      }
      await new Promise((resolve) => setTimeout(resolve, 1_000));
    }
  }
}

export function createLiveVoiceEditTransport(): VoiceEditTransportPort {
  return {
    transcribe: (r) => sendVoiceTranscribe(r),
    plan: (r) => sendVoicePlan(r),
    sendEditStats: (row) => sendEditStats(row),
  };
}
