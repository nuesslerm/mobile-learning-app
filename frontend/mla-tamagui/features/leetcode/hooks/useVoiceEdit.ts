import { Logger } from '@lib/logging';
import {
  parseLocalEdit,
  editSizeLimitsFor,
  validateAlternative,
  type ClientEditStat,
  type EditAlternative,
  type EditSource,
  type VoiceEditResponse,
} from '@lib/voice-edit-dsl';
import { useCallback, useRef, useState } from 'react';
import { Platform } from 'react-native';

import type { Timed } from '../contracts/ports';
import { useLeetCodePorts } from '../di/LeetCodeContainer';
import {
  compactVoiceError,
  errorStatus,
  newEditId,
} from '../data/voiceEditIdentity';

import { selectAutoApplyAlternative } from './selectAutoApplyAlternative';
import { useVoiceRecording, type VoiceRecording } from './useVoiceRecording';

type VoiceEditPhase =
  | 'idle'
  | 'recording'
  | 'transcribing'
  | 'reviewing'
  | 'planning'
  | 'choosing'
  | 'applied'
  | 'error';

interface VoiceEditState {
  phase: VoiceEditPhase;
  /** How this turn's instruction is produced: spoken and transcribed, or typed. Null while idle. */
  source: EditSource | null;
  /** One id per turn, shared with the server's rows for the same turn. Null while idle. */
  editId: string | null;
  /** The just-recorded clip's local URI, kept through transcribing/reviewing so it can be played back. */
  recordingUri: string | null;
  /** Whisper's transcript, then the user's own edits on top of it while phase is 'reviewing'. */
  transcript: string | null;
  alternatives: EditAlternative[];
  error: string | null;
  /** The alternative that should be auto-applied when the phase becomes 'applied'. */
  autoApply: EditAlternative | null;
}

export interface UseVoiceEditResult {
  state: VoiceEditState;
  /** Current recording duration, updated while phase is 'recording'. */
  durationMs: number;
  /** Real mic-level samples (0-1) while phase is 'recording' -- see useVoiceRecording. */
  amplitudes: number[];
  start: () => Promise<void>;
  /**
   * Opens the review sheet for a typed instruction: no recording, no
   * transcription, straight to 'reviewing' with an empty (or given) text.
   * Send then takes exactly the path a spoken transcript takes.
   */
  startTextEdit: (initialText?: string) => void;
  /** Stops recording and moves to 'transcribing' -> 'reviewing'. Does NOT engage the LLM. */
  stop: () => Promise<void>;
  /** Edits the transcript shown in the review sheet, before it's sent for planning. */
  updateTranscript: (text: string) => void;
  /** Sends the (possibly edited) transcript for LLM planning. Only valid while phase is 'reviewing'. */
  confirmSend: () => Promise<void>;
  /** Discards the current recording/transcript and returns to 'idle' so the user can record again. */
  reRecord: () => void;
  cancel: () => void;
  dismiss: () => void;
  consumeAutoApply: () => EditAlternative | null;
  /**
   * The screen applied an alternative -- from the picker (`pickedIndex`) or
   * the auto-apply -- producing `codeAfter`. Recorded on the turn's stats row;
   * call it before `dismiss`.
   */
  markApplied: (applied: { pickedIndex?: number; codeAfter: string }) => void;
}

/**
 * `filename` is caller-supplied with a fixed extension (e.g. `two-sum.m4a`),
 * but the real container varies by platform (web records webm, not m4a) --
 * matching the extension to the recording's own mimeType keeps the upload's
 * metadata honest even though ffmpeg itself sniffs content, not the name.
 */
function filenameForMimeType(filename: string, mimeType: string): string {
  const ext = mimeType.split('/')[1];
  if (!ext) return filename;
  return `${filename.replace(/\.[^./]+$/, '')}.${ext}`;
}

const EMPTY_STATE: VoiceEditState = {
  phase: 'idle',
  source: null,
  editId: null,
  recordingUri: null,
  transcript: null,
  alternatives: [],
  error: null,
  autoApply: null,
};

/**
 * The transport adapter normalizes network failures into a
 * `VoiceEditTransportError` carrying a `status`; anything else the port
 * rejects with is a plain error or an unexpected value.
 */
function errorMessage(err: unknown): string {
  const status = (err as { status?: unknown } | null)?.status;
  if (err instanceof Error && typeof status === 'number') {
    return `Server error (${status}): ${err.message}`;
  }
  return err instanceof Error ? err.message : String(err);
}

/**
 * The client half of one turn's stats row, accumulated as the turn runs and
 * sent once when it ends (applied, picked, dismissed, cancelled, error). Kept
 * in a ref, not state: it never renders anything.
 */
type TurnStats = Omit<ClientEditStat, 'outcome' | 'error'> & {
  /** True once a row was sent for this turn -- a turn ends exactly once. */
  sent: boolean;
};

export function useVoiceEdit(
  currentCode: string,
  language: string,
  filename: string,
  context: { problemId?: string | null } = {}
): UseVoiceEditResult {
  const { voice } = useLeetCodePorts();
  const recording = useVoiceRecording();
  const [state, setState] = useState<VoiceEditState>(EMPTY_STATE);
  const pendingAutoApplyRef = useRef<EditAlternative | null>(null);
  const turnStatsRef = useRef<TurnStats | null>(null);
  const problemId = context.problemId ?? null;

  const beginTurn = useCallback(
    (source: EditSource): string => {
      const editId = newEditId();
      turnStatsRef.current = {
        editId,
        source,
        problemId,
        platform: Platform.OS,
        transcribeWallMs: null,
        planWallMs: null,
        postConfirmWaitMs: null,
        speculativeHit: false,
        localFastPath: false,
        pickedIndex: null,
        alternatives: 0,
        transcriptChars: null,
        codeCharsBefore: currentCode.length,
        codeCharsAfter: null,
        sent: false,
      };
      return editId;
    },
    [currentCode.length, problemId]
  );

  const endTurn = useCallback(
    (outcome: ClientEditStat['outcome'], error: string | null = null) => {
      const turn = turnStatsRef.current;
      if (!turn || turn.sent) return;
      turn.sent = true;
      const { sent: _sent, ...fields } = turn;
      void voice.sendEditStats({ ...fields, outcome, error });
    },
    [voice]
  );
  /**
   * A plan already in flight for a transcript the user has not confirmed yet.
   *
   * The review step is dead time for the backend: whisper has answered, the
   * user is reading, and nothing is being computed. Planning is also the
   * slowest call in the flow by a wide margin -- measured against the deployed
   * planner, transcription is a steady 12-13s while the same planning request
   * took 20.5s, 37.5s and 75.7s on three identical runs. Starting it the
   * moment the transcript arrives moves that whole call off the critical path
   * whenever the user accepts what whisper heard, which is the common case.
   *
   * Keyed by the exact transcript and code it was computed for, so an edited
   * transcript falls back to a fresh request rather than quietly planning
   * against words the user rejected.
   */
  const speculativePlanRef = useRef<{
    transcript: string;
    code: string;
    language: string;
    promise: Promise<Timed<VoiceEditResponse>>;
  } | null>(null);

  const start = useCallback(async () => {
    pendingAutoApplyRef.current = null;
    speculativePlanRef.current = null;
    const editId = beginTurn('voice');
    Logger.info('voice_edit.start', { codeLength: currentCode.length, editId });
    setState({ ...EMPTY_STATE, phase: 'recording', source: 'voice', editId });
    await recording.startRecording();
  }, [currentCode.length, recording, beginTurn]);

  const startTextEdit = useCallback(
    (initialText = '') => {
      pendingAutoApplyRef.current = null;
      speculativePlanRef.current = null;
      const editId = beginTurn('text');
      Logger.info('voice_edit.text_start', {
        codeLength: currentCode.length,
        editId,
      });
      setState({
        ...EMPTY_STATE,
        phase: 'reviewing',
        source: 'text',
        editId,
        transcript: initialText,
      });
    },
    [currentCode.length, beginTurn]
  );

  const runTranscribe = useCallback(
    async (voiceRecording: VoiceRecording | null) => {
      const editId = turnStatsRef.current?.editId;
      if (!voiceRecording) {
        Logger.warn('voice_edit.empty_recording', { editId });
        setState({
          ...EMPTY_STATE,
          phase: 'error',
          error: 'Recording was empty.',
        });
        return;
      }

      setState((prev) => ({
        ...prev,
        phase: 'transcribing',
        recordingUri: voiceRecording.uri,
      }));
      const startedAt = Date.now();
      Logger.info('voice_edit.transcribe.send', {
        durationMs: voiceRecording.durationMs,
        editId,
      });

      try {
        const { transcript, wallMs } = await voice.transcribe({
          audioUri: voiceRecording.uri,
          mimeType: voiceRecording.mimeType,
          filename: filenameForMimeType(filename, voiceRecording.mimeType),
          editId,
        });
        if (turnStatsRef.current) {
          turnStatsRef.current.transcribeWallMs = wallMs;
          turnStatsRef.current.transcriptChars = transcript.length;
        }
        Logger.info('voice_edit.transcribe.response', {
          transcriptLength: transcript.length,
          ms: Date.now() - startedAt,
          wallMs,
          editId,
        });
        // Fire the plan now, against whisper's own words. `.catch` here only
        // stops an unhandled rejection while nobody is awaiting it; the real
        // error handling still happens in confirmSend, which re-throws it.
        const speculative = voice.plan({
          transcript,
          currentCode,
          language,
          source: 'voice',
          editId,
        });
        speculative.catch(() => undefined);
        speculativePlanRef.current = {
          transcript,
          code: currentCode,
          language,
          promise: speculative,
        };
        Logger.info('voice_edit.plan.speculative_start', {
          transcriptLength: transcript.length,
        });
        setState((prev) => ({
          ...prev,
          phase: 'reviewing',
          transcript,
        }));
      } catch (err) {
        const message = errorMessage(err);
        Logger.withError(err)
          .withMetadata({
            ms: Date.now() - startedAt,
            status: errorStatus(err),
            editId,
          })
          .error('voice_edit.transcribe.error');
        endTurn('error', message);
        setState({ ...EMPTY_STATE, phase: 'error', error: message });
      }
    },
    [filename, currentCode, language, endTurn, voice]
  );

  const stop = useCallback(async () => {
    if (!recording.isRecording) return;
    Logger.info('voice_edit.stop', { durationMs: recording.durationMs });
    const voiceRecording = await recording.stopRecording();
    await runTranscribe(voiceRecording);
  }, [recording, runTranscribe]);

  const updateTranscript = useCallback((text: string) => {
    setState((prev) =>
      prev.phase === 'reviewing' ? { ...prev, transcript: text } : prev
    );
  }, []);

  const confirmSend = useCallback(async () => {
    const transcript = state.transcript;
    if (state.phase !== 'reviewing' || !transcript) return;

    // A rename needs no planner: the transcript names both symbols and the
    // buffer says where the old one is. Skipping the round trip is the
    // difference between an edit that lands now and one that lands in ten
    // seconds -- and this is the edit a speaker repeats most.
    const local = parseLocalEdit(transcript, currentCode);
    if (local) {
      Logger.info('voice_edit.plan.local', {
        commandTypes: local.commands.map((c) => c.type),
        editId: turnStatsRef.current?.editId,
      });
      speculativePlanRef.current = null;
      pendingAutoApplyRef.current = local;
      if (turnStatsRef.current) {
        turnStatsRef.current.localFastPath = true;
        turnStatsRef.current.transcriptChars = transcript.length;
        turnStatsRef.current.alternatives = 1;
      }
      setState((prev) => ({
        ...prev,
        phase: 'applied',
        transcript,
        alternatives: [local],
        autoApply: local,
      }));
      return;
    }

    setState((prev) => ({ ...prev, phase: 'planning' }));
    const startedAt = Date.now();
    const source = state.source ?? 'voice';
    const editId = turnStatsRef.current?.editId;
    Logger.info('voice_edit.plan.send', {
      codeLength: currentCode.length,
      language,
      transcriptLength: transcript.length,
      // `source` is the logger's own module field; the edit's origin needs another name.
      editSource: source,
      editId,
    });

    try {
      // Reuse the speculative plan only when it was computed for exactly these
      // words and this buffer -- an edited transcript means the user rejected
      // what it was planning from.
      const pending = speculativePlanRef.current;
      const reusable =
        pending !== null &&
        pending.transcript === transcript &&
        pending.code === currentCode &&
        pending.language === language;
      speculativePlanRef.current = null;
      Logger.info('voice_edit.plan.speculative_hit', { reused: reusable });
      const response = await (reusable
        ? pending.promise
        : voice.plan({
            transcript,
            currentCode,
            language,
            source,
            editId,
          }));
      const postConfirmWaitMs = Date.now() - startedAt;
      if (turnStatsRef.current) {
        turnStatsRef.current.planWallMs = response.wallMs;
        turnStatsRef.current.postConfirmWaitMs = postConfirmWaitMs;
        turnStatsRef.current.speculativeHit = reusable;
        turnStatsRef.current.transcriptChars = transcript.length;
        turnStatsRef.current.alternatives = response.alternatives.length;
      }
      Logger.info('voice_edit.plan.response', {
        alternatives: response.alternatives.length,
        ms: postConfirmWaitMs,
        wallMs: response.wallMs,
        editSource: source,
        editId,
      });
      // The backend validates each alternative against the code it was sent,
      // with whatever version of the DSL rules that deployment carries. The
      // client holds the actual buffer and the current rules, so its verdict
      // is the one that decides what can be applied -- otherwise an
      // alternative this app could apply perfectly well stays greyed out
      // until the backend ships, which is the difference between a voice
      // session that can continue and one that is stuck.
      const alternatives = response.alternatives.map((alt) => {
        const validated = validateAlternative(alt, currentCode, editSizeLimitsFor(transcript));
        return validated;
      });
      const rescued = alternatives.filter(
        (alt, i) => alt.valid && response.alternatives[i]?.valid === false
      ).length;
      if (rescued > 0) {
        Logger.info('voice_edit.plan.revalidated', { rescued });
      }
      if (alternatives.every((alt) => alt.valid === false)) {
        const reason = alternatives.find((alt) => alt.invalidReason)
          ?.invalidReason;
        Logger.warn('voice_edit.plan.none_valid', {
          editId,
          alternatives: alternatives.length,
          invalidReason: reason ? compactVoiceError(reason) : undefined,
        });
      }

      const autoApply = selectAutoApplyAlternative(alternatives);

      if (autoApply) {
        pendingAutoApplyRef.current = autoApply;
        setState((prev) => ({
          ...prev,
          phase: 'applied',
          transcript: response.transcript,
          alternatives,
          autoApply,
        }));
      } else {
        setState((prev) => ({
          ...prev,
          phase: 'choosing',
          transcript: response.transcript,
          alternatives,
        }));
      }
    } catch (err) {
      const message = errorMessage(err);
      Logger.withError(err)
        .withMetadata({
          ms: Date.now() - startedAt,
          status: errorStatus(err),
          editId,
        })
        .error('voice_edit.plan.error');
      endTurn('error', message);
      setState({ ...EMPTY_STATE, phase: 'error', error: message });
    }
  }, [
    state.phase,
    state.transcript,
    state.source,
    currentCode,
    language,
    endTurn,
    voice,
  ]);

  const reRecord = useCallback(() => {
    speculativePlanRef.current = null;
    Logger.info('voice_edit.re_record', {});
    endTurn('cancelled');
    setState(EMPTY_STATE);
  }, [endTurn]);

  const cancel = useCallback(() => {
    speculativePlanRef.current = null;
    Logger.info('voice_edit.cancel', {});
    if (recording.isRecording) {
      void recording.stopRecording();
    }
    pendingAutoApplyRef.current = null;
    endTurn('cancelled');
    setState(EMPTY_STATE);
  }, [recording, endTurn]);

  const markApplied = useCallback(
    (applied: { pickedIndex?: number; codeAfter: string }) => {
      const turn = turnStatsRef.current;
      if (!turn) return;
      if (applied.pickedIndex !== undefined)
        turn.pickedIndex = applied.pickedIndex;
      turn.codeCharsAfter = applied.codeAfter.length;
    },
    []
  );

  // `dismiss` closes every terminal phase: after an auto-apply, after a pick,
  // when the picker is closed without choosing, when the error banner goes.
  // Which of those it was is what the stats row records.
  const dismiss = useCallback(() => {
    speculativePlanRef.current = null;
    Logger.info('voice_edit.dismiss', {});
    pendingAutoApplyRef.current = null;
    const turn = turnStatsRef.current;
    if (turn && !turn.sent) {
      const outcome: ClientEditStat['outcome'] =
        turn.pickedIndex !== null
          ? 'picked'
          : turn.codeCharsAfter !== null
            ? 'auto'
            : 'dismissed';
      endTurn(outcome);
    }
    setState(EMPTY_STATE);
  }, [endTurn]);

  const consumeAutoApply = useCallback(() => {
    const alt = pendingAutoApplyRef.current;
    pendingAutoApplyRef.current = null;
    return alt;
  }, []);

  return {
    state,
    durationMs: recording.durationMs,
    amplitudes: recording.amplitudes,
    start,
    startTextEdit,
    stop,
    updateTranscript,
    confirmSend,
    reRecord,
    cancel,
    dismiss,
    consumeAutoApply,
    markApplied,
  };
}
