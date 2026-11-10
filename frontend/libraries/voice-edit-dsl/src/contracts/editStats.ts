import { z } from 'zod';

/**
 * One edit turn as the client experienced it, posted to `POST /api/edit-stats`
 * when the turn ends. The server writes it as a `side = 'client'` row next to
 * its own `side = 'server'` rows for the same `editId`.
 */
export const editOutcomeSchema = z.enum([
  'auto',
  'picked',
  'dismissed',
  'error',
  'cancelled',
]);
export type EditOutcome = z.infer<typeof editOutcomeSchema>;

export const editSourceSchema = z.enum([
  'voice',
  'text',
  'plan-voice',
  'plan-text',
]);
export type EditSource = z.infer<typeof editSourceSchema>;

const ms = z.number().int().min(0).max(3_600_000);

export const clientEditStatSchema = z.object({
  editId: z.string().min(1).max(64),
  source: editSourceSchema,
  /**
   * Which call this row measures. Absent means the client's whole turn, which
   * is what every row was before live transcription: a chunk sends its own
   * row, many of them per `editId`, and only `chunkIndex` tells them apart.
   */
  route: z.enum(['turn', 'transcribe-chunk']).optional(),
  chunkIndex: z.number().int().min(0).max(10_000).nullish(),
  problemId: z.string().max(128).nullable(),
  platform: z.string().max(16),
  /** Recording stop -> transcript parsed; null for typed edits and for turns that never got that far. */
  transcribeWallMs: ms.nullable(),
  /** True duration of the plan HTTP call, speculative or not; null when no plan call was made. */
  planWallMs: ms.nullable(),
  /** What the user actually waited after confirming: near zero on a speculative hit, the whole call otherwise. */
  postConfirmWaitMs: ms.nullable(),
  speculativeHit: z.boolean(),
  localFastPath: z.boolean(),
  outcome: editOutcomeSchema,
  pickedIndex: z.number().int().min(0).max(99).nullable(),
  alternatives: z.number().int().min(0).max(99),
  transcriptChars: z.number().int().min(0).max(100_000).nullable(),
  codeCharsBefore: z.number().int().min(0).max(10_000_000),
  codeCharsAfter: z.number().int().min(0).max(10_000_000).nullable(),
  error: z.string().max(1000).nullable(),
});
export type ClientEditStat = z.infer<typeof clientEditStatSchema>;

/**
 * A row of HEALTH_RPT.EDIT_EVENTS as ORDS auto-REST accepts and returns it
 * (lower-case column names; Oracle has no boolean, so flags are 0/1). Every
 * field except `edit_id`, `side` and `route` is nullable: a row records what a
 * call knew, never a guess.
 */
export type EditEventRow = {
  edit_id: string;
  side: 'client' | 'server';
  /**
   * Which call: 'transcribe' | 'plan' | 'edit' (the combined route) | 'turn'
   * (the client's whole turn) | 'transcribe-chunk' (one clip of a live
   * transcription, numbered by `chunk_index`).
   */
  route: 'transcribe' | 'plan' | 'edit' | 'turn' | 'transcribe-chunk';
  source: EditSource | null;
  ok: 0 | 1;
  status: number | null;
  error_code: string | null;
  ms: number | null;
  ffmpeg_ms: number | null;
  whisper_ms: number | null;
  whisper_model: string | null;
  whisper_threads: number | null;
  audio_bytes: number | null;
  transcript_chars: number | null;
  planner_model: string | null;
  planner_ms: number | null;
  session_reused: 0 | 1 | null;
  prompt_tokens: number | null;
  cache_read: number | null;
  cache_write: number | null;
  alternatives: number | null;
  invalid_count: number | null;
  retried: 0 | 1 | null;
  code_chars: number | null;
  language: string | null;
  host: string | null;
  problem_id: string | null;
  platform: string | null;
  transcribe_wall_ms: number | null;
  plan_wall_ms: number | null;
  post_confirm_wait_ms: number | null;
  speculative_hit: 0 | 1 | null;
  local_fast_path: 0 | 1 | null;
  outcome: EditOutcome | null;
  picked_index: number | null;
  code_chars_after: number | null;
  /** Which clip of a live transcription this row measures; null for every other route. */
  chunk_index: number | null;
  error_text: string | null;
};

/** Every column, in table order -- the DDL, the writer and the reader agree through this list. */
export const EDIT_EVENT_COLUMNS = [
  'edit_id',
  'side',
  'route',
  'source',
  'ok',
  'status',
  'error_code',
  'ms',
  'ffmpeg_ms',
  'whisper_ms',
  'whisper_model',
  'whisper_threads',
  'audio_bytes',
  'transcript_chars',
  'planner_model',
  'planner_ms',
  'session_reused',
  'prompt_tokens',
  'cache_read',
  'cache_write',
  'alternatives',
  'invalid_count',
  'retried',
  'code_chars',
  'language',
  'host',
  'problem_id',
  'platform',
  'transcribe_wall_ms',
  'plan_wall_ms',
  'post_confirm_wait_ms',
  'speculative_hit',
  'local_fast_path',
  'outcome',
  'picked_index',
  'code_chars_after',
  'chunk_index',
  'error_text',
] as const satisfies readonly (keyof EditEventRow)[];

const flag = (value: boolean | null | undefined): 0 | 1 | null =>
  value === null || value === undefined ? null : value ? 1 : 0;

/** A fully-null row with the three required fields set; callers overwrite what they know. */
export function emptyEditEventRow(
  editId: string,
  side: EditEventRow['side'],
  route: EditEventRow['route']
): EditEventRow {
  const row = Object.fromEntries(
    EDIT_EVENT_COLUMNS.map((c) => [c, null])
  ) as unknown as EditEventRow;
  row.edit_id = editId;
  row.side = side;
  row.route = route;
  row.ok = 1;
  return row;
}

/** The client's turn record as a table row. */
export function clientEditStatToRow(
  stat: ClientEditStat,
  host: string | null
): EditEventRow {
  const row = emptyEditEventRow(stat.editId, 'client', stat.route ?? 'turn');
  row.source = stat.source;
  row.chunk_index = stat.chunkIndex ?? null;
  row.ok = stat.outcome === 'error' ? 0 : 1;
  row.problem_id = stat.problemId;
  row.platform = stat.platform;
  row.transcribe_wall_ms = stat.transcribeWallMs;
  row.plan_wall_ms = stat.planWallMs;
  row.post_confirm_wait_ms = stat.postConfirmWaitMs;
  row.speculative_hit = flag(stat.speculativeHit);
  row.local_fast_path = flag(stat.localFastPath);
  row.outcome = stat.outcome;
  row.picked_index = stat.pickedIndex;
  row.alternatives = stat.alternatives;
  row.transcript_chars = stat.transcriptChars;
  row.code_chars = stat.codeCharsBefore;
  row.code_chars_after = stat.codeCharsAfter;
  row.error_text = stat.error;
  row.host = host;
  return row;
}

export { flag as editEventFlag };
