import { Logger } from '@lib/logging';
import { useCallback, useEffect, useRef, useState } from 'react';
import { Platform } from 'react-native';

import { errorStatus } from '../../data/voiceEditIdentity';
import { useLeetCodePorts } from '../../di/LeetCodeContainer';
import { useVoiceRecording } from '../../state/useVoiceRecording';

const DEFAULT_CHUNK_MS = 1500;

export interface LiveTranscriptOptions {
  /** Shared by every clip of this session; one turn's edit id. */
  editId: string;
  /** How long each recording runs before it is cut and posted. */
  chunkMs?: number;
}

export interface LiveTranscriptResult {
  listening: boolean;
  /** Chunk transcripts joined by single spaces, strictly in chunk order. */
  text: string;
  start: () => Promise<void>;
  stop: () => Promise<void>;
  /** Wall time (ms) of each chunk's HTTP call, in chunk order. */
  chunkMs: number[];
}

/**
 * Records the mic in fixed-length chunks while the user talks: every
 * `chunkMs` the recorder is stopped, the clip is posted for transcription,
 * and recording restarts immediately. Transcripts are appended in chunk
 * order even when the backend answers out of order.
 */
export function useLiveTranscript({
  editId,
  chunkMs = DEFAULT_CHUNK_MS,
}: LiveTranscriptOptions): LiveTranscriptResult {
  const { startRecording, stopRecording } = useVoiceRecording();
  const [listening, setListening] = useState(false);
  const [text, setText] = useState('');
  const { voice } = useLeetCodePorts();
  const [wallTimes, setWallTimes] = useState<number[]>([]);

  const listeningRef = useRef(false);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const cuttingRef = useRef(false);
  const nextIndexRef = useRef(0);
  const nextToAppendRef = useRef(0);
  const bufferRef = useRef(
    new Map<number, { transcript: string; wallMs: number }>()
  );
  const pendingRef = useRef<Promise<void>[]>([]);
  const textRef = useRef('');

  const clearTimer = useCallback(() => {
    if (intervalRef.current !== null) {
      clearInterval(intervalRef.current);
      intervalRef.current = null;
    }
  }, []);

  useEffect(() => clearTimer, [clearTimer]);

  const appendTranscript = useCallback(
    (index: number, transcript: string, wallMs: number) => {
      bufferRef.current.set(index, { transcript, wallMs });
      while (bufferRef.current.has(nextToAppendRef.current)) {
        const entry = bufferRef.current.get(nextToAppendRef.current);
        if (!entry) break;
        bufferRef.current.delete(nextToAppendRef.current);
        nextToAppendRef.current += 1;
        textRef.current =
          textRef.current.length > 0
            ? `${textRef.current} ${entry.transcript}`
            : entry.transcript;
        setText(textRef.current);
        setWallTimes((prev) => [...prev, entry.wallMs]);
      }
    },
    []
  );

  /**
   * One stats row per clip, so the live-words gate is answerable from
   * EDIT_EVENTS rather than from logs: every row shares the turn's `editId`
   * and is told apart by its chunk index.
   */
  const recordChunkStat = useCallback(
    (
      index: number,
      wallMs: number | null,
      transcriptChars: number | null,
      error: string | null
    ) => {
      void voice.sendEditStats({
        editId,
        source: 'plan-voice',
        route: 'transcribe-chunk',
        chunkIndex: index,
        problemId: null,
        platform: Platform.OS,
        transcribeWallMs: wallMs,
        planWallMs: null,
        postConfirmWaitMs: null,
        speculativeHit: false,
        localFastPath: false,
        outcome: error === null ? 'auto' : 'error',
        pickedIndex: null,
        alternatives: 0,
        transcriptChars,
        codeCharsBefore: 0,
        codeCharsAfter: null,
        error: error === null ? null : error.slice(0, 300),
      });
    },
    [editId, voice]
  );

  const cutChunk = useCallback(async (): Promise<void> => {
    if (cuttingRef.current) return;
    cuttingRef.current = true;
    const index = nextIndexRef.current;
    nextIndexRef.current += 1;
    try {
      const clip = await stopRecording();
      if (listeningRef.current) {
        void startRecording().catch((err) => {
          Logger.withError(err)
            .withMetadata({ chunkIndex: index, editId })
            .error('live_transcript.chunk.error');
        });
      }
      if (!clip) {
        Logger.warn('live_transcript.chunk.empty', {
          chunkIndex: index,
          editId,
        });
        return;
      }

      const ext = clip.mimeType.split('/')[1] ?? 'webm';
      Logger.withMetadata({ chunkIndex: index, editId }).info(
        'live_transcript.chunk.sent'
      );
      const pending = voice.transcribe({
        audioUri: clip.uri,
        mimeType: clip.mimeType,
        filename: `${editId}-${index}.${ext}`,
        editId,
        chunkIndex: index,
      })
        .then((res) => {
          Logger.withMetadata({
            chunkIndex: index,
            wallMs: res.wallMs,
            transcriptLength: res.transcript.length,
            editId,
          }).info('live_transcript.chunk.received');
          appendTranscript(index, res.transcript, res.wallMs);
          void recordChunkStat(index, res.wallMs, res.transcript.length, null);
        })
        .catch((err) => {
          Logger.withError(err)
            .withMetadata({
              chunkIndex: index,
              editId,
              status: errorStatus(err),
            })
            .error('live_transcript.chunk.error');
          void recordChunkStat(index, null, null, String(err));
        });
      pendingRef.current.push(pending);
    } catch (err) {
      Logger.withError(err)
        .withMetadata({ chunkIndex: index, editId })
        .error('live_transcript.chunk.error');
    } finally {
      cuttingRef.current = false;
    }
  }, [
    appendTranscript,
    editId,
    recordChunkStat,
    startRecording,
    stopRecording,
    voice,
  ]);

  const start = useCallback(async () => {
    if (listeningRef.current) return;
    listeningRef.current = true;
    setListening(true);
    textRef.current = '';
    setText('');
    setWallTimes([]);
    nextIndexRef.current = 0;
    nextToAppendRef.current = 0;
    bufferRef.current.clear();
    pendingRef.current = [];

    try {
      await startRecording();
    } catch (err) {
      Logger.withError(err)
        .withMetadata({ editId })
        .error('live_transcript.start_failed');
      throw err;
    }
    intervalRef.current = setInterval(() => {
      void cutChunk();
    }, chunkMs);
  }, [chunkMs, cutChunk, editId, startRecording]);

  const stop = useCallback(async () => {
    if (!listeningRef.current) return;
    listeningRef.current = false;
    clearTimer();
    setListening(false);

    await cutChunk();
    const pending = pendingRef.current;
    pendingRef.current = [];
    await Promise.all(pending);
  }, [clearTimer, cutChunk]);

  return { listening, text, start, stop, chunkMs: wallTimes };
}
