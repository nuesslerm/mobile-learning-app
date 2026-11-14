// Pure ports + the moved type definitions they reference. No imports outside
// `contracts/` and `@lib/*-contracts`; type definitions were moved here from
// their runtime modules (data/, hooks/, code-editor/) so this layer stays
// dependency-free.

import type { RunResponse, TestCase } from './execution';
import type {
  ClientEditStat,
  EditSource,
  VoiceEditResponse,
} from '@lib/voice-edit-dsl';

// Moved from data/webBridgeClient.ts (was a local type).
export type UserStatusResult = { isSignedIn: boolean; username: string | null };

// Mirrors data/webBridgeClient.ts fetch* params minus backendBaseUrl.
type TestParams = {
  slug: string;
  lang: string;
  questionId: number;
  code: string;
  dataInput: string;
};
type SubmitParams = {
  slug: string;
  lang: string;
  questionId: number;
  code: string;
};
type RunLocalParams = {
  lang: string;
  code: string;
  methodName: string;
  testCases: TestCase[];
};
type FormatParams = { lang: string; code: string };

// Moved from data/webBridgeClient.ts.
export type LoginCaptureStatus = { active: boolean; expiresAt: number | null };

export interface LeetCodeGatewayPort {
  // mirrors data/webBridgeClient.ts exports minus backendBaseUrl
  userStatus(): Promise<UserStatusResult>;
  test(p: TestParams): Promise<RunResponse>;
  submit(p: SubmitParams): Promise<RunResponse>;
  poll(id: string): Promise<RunResponse>;
  runLocal(p: RunLocalParams): Promise<RunResponse>;
  format(p: FormatParams): Promise<string>;
  loginCaptureStatus(): Promise<LoginCaptureStatus>;
}

// Moved from data/voiceEditTransport.ts.
type VoiceTranscribeRequest = {
  audioUri: string;
  mimeType: string;
  filename: string;
  /** Zero-based position of this clip inside a live-transcript session. */
  chunkIndex?: number;
};
type VoiceTranscribeResponse = { transcript: string };
type VoicePlanRequest = {
  transcript: string;
  currentCode: string;
  language: string;
  /** How the instruction was produced. The server stores it with the row; nothing else reads it. */
  source?: EditSource;
  /** One id per edit turn, shared by the transcribe call, the plan call and the stats row. */
  editId?: string;
};
type VoicePlanResponse = VoiceEditResponse;

/** Wall time of the whole HTTP call as the client saw it -- request sent to body parsed. */
export type Timed<T> = T & { wallMs: number };

export interface VoiceEditTransportPort {
  // mirrors data/voiceEditTransport.ts
  transcribe(r: VoiceTranscribeRequest & { editId?: string }): Promise<Timed<VoiceTranscribeResponse>>;
  plan(r: VoicePlanRequest): Promise<Timed<VoicePlanResponse>>;
  sendEditStats(row: ClientEditStat): Promise<void>;
}

// Moved from hooks/useVoiceRecording.ts.
export type VoiceRecording = { uri: string; mimeType: string; durationMs: number };

export interface AudioRecordingPort {
  // replaces direct expo-audio use in hooks/useVoiceRecording.ts
  requestPermission(): Promise<boolean>;
  start(): Promise<void>;
  stop(): Promise<VoiceRecording | null>;
  subscribe(l: (s: { isRecording: boolean; durationMs: number; level: number }) => void): () => void;
}

// Moved from code-editor/core/EditorMessages.ts.
export type EditorConfig = {
  initialText: string;
  isDark: boolean;
  fontSize: number;
  lineHeight: number;
  wordWrap: boolean;
  language: string;
  cursorColor: string;
  selectionColor: string;
  backgroundColor: string;
  gutterColor: string;
  borderColor: string;
  tabSize: number;
  noAutoFocus?: boolean;
};

export type ToEditorMessage =
  | { type: 'init'; config: EditorConfig }
  | { type: 'updateConfig'; config: Partial<EditorConfig> }
  | { type: 'focus' }
  | { type: 'setSelection'; start: number; end: number }
  | { type: 'insertAt'; text: string; offset?: number }
  | { type: 'typeText'; text: string }
  | { type: 'undo' }
  | { type: 'redo' }
  | { type: 'setText'; text: string }
  | { type: 'blur' }
  | { type: 'highlightRange'; start: number; end: number; durationMs?: number }
  | { type: 'clearHighlights' }
  | { type: 'showDiff'; start: number; end: number }
  | { type: 'hideDiff' };

type EditorSelection = { start: number; end: number };

// Mirrors the webview's postMessage protocol (see code-editor/cm6Bundle.src.ts).
export type FromEditorMessage =
  | { type: 'ready' }
  | { type: 'hydrated'; text: string; selection: EditorSelection }
  | { type: 'focused' }
  | { type: 'blurred' }
  | { type: 'log'; message: string }
  | { type: 'error'; message: string }
  | { type: 'contentChanged'; text: string; selection: EditorSelection }
  | { type: 'selectionChanged'; selection: EditorSelection };

export interface CodeEditorPort {
  post(msg: ToEditorMessage): void;
  onMessage(l: (m: FromEditorMessage) => void): () => void;
}

export interface LeetCodePorts {
  gateway: LeetCodeGatewayPort;
  voice: VoiceEditTransportPort;
  audio: AudioRecordingPort;
}

export class VoiceEditTransportError extends Error {
  constructor(
    public readonly status: number,
    message: string
  ) {
    super(message);
    this.name = 'VoiceEditTransportError';
  }
}
