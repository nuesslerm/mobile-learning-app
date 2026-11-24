// Pure ports + the moved type definitions they reference. No imports outside
// `contracts/` and `@lib/*-contracts`; type definitions were moved here from
// their runtime modules (data/, state/, ui/) so this layer stays
// dependency-free.

import type { EditorClient } from '@lib/code-editor-api';
import type { LibraryContentPort, LibraryCoverPort, LibraryStoragePort } from './library/library';
import type { LeetCodeFlags } from './featureFlags';
import type { RunResponse, TestCase } from './run/execution';
import type { Harness, NodeType } from './models';
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
export type RunCodeParams = {
  lang: string;
  code: string;
  methodName: string;
  testCases: TestCase[];
  /** How a runner that holds real objects builds a linked-list or tree argument from its JSON form; the backend ignores these three. */
  paramTypes?: Record<string, NodeType>;
  returnType?: NodeType | null;
  /** The parameter the solution mutates instead of returning: that value is what is compared. */
  inPlace?: string | null;
  /** A problem that is a class, not a `Solution` method: only the device runner can run it. */
  harness?: Harness | null;
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
  runLocal(p: RunCodeParams): Promise<RunResponse>;
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

export type EditorConfig = {
  initialText: string;
  isDark: boolean;
  fontSize: number;
  lineHeight: number;
  wordWrap: boolean;
  showWhitespace?: boolean;
  language: string;
  cursorColor: string;
  selectionColor: string;
  backgroundColor: string;
  gutterColor: string;
  borderColor: string;
  tabSize: number;
  noAutoFocus?: boolean;
  surface?: 'canvas';
  syntaxPalette?: 'default' | 'contrast';
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
  | { type: 'blur' };

type EditorSelection = { start: number; end: number };

// Mirrors the webview's postMessage protocol (see data/editor/cm6/cm6Bundle.src.ts).
export type FromEditorMessage =
  | { type: 'ready'; evalMs?: number }
  | { type: 'hydrated'; text: string; selection: EditorSelection }
  | { type: 'focused' }
  | { type: 'blurred' }
  | { type: 'log'; message: string }
  | { type: 'error'; message: string }
  | { type: 'contentChanged'; text: string; selection: EditorSelection; canUndo: boolean }
  | { type: 'selectionChanged'; selection: EditorSelection }
  | { type: 'inputTiming'; inputToUpdateMs: number | null; updateToFrameMs: number; updates: number; sentAtMs: number };

/** What the editor view hands the page it shows: a WebView that takes scripts. */
export type WebViewHandle = { injectJavaScript(script: string): void };

/**
 * The page the code editor runs in and the typed editor API over it: `html` is what the WebView loads; `connect` gives the client that
 * makes the calls and `receive`, which takes each raw message the WebView posts back.
 */
export interface EditorBridgePort {
  html(): string;
  connect(webView: () => WebViewHandle | null): { client: EditorClient; receive(raw: string): void; close(): void };
}

export interface CodeEditorPort {
  post(msg: ToEditorMessage): void;
  onMessage(l: (m: FromEditorMessage) => void): () => void;
}

/**
 * Runs a solution against a problem's test cases and says, per case, what went in, what was expected and what came
 * out. A runner is wherever the code runs: the leetcode-backend through the gateway, or the device itself.
 */
export interface CodeRunnerPort {
  run(p: RunCodeParams): Promise<RunResponse>;
  /** Starts what the runner needs before its first run, for the editor language it was asked about; a runner that needs nothing leaves it out. */
  warmUp?(lang?: string): void;
}

/** Where Run Local executes: the leetcode-backend, or this device. */
export type RunnerChoice = 'backend' | 'device';

export type CodeRunnerStage = 'assets' | 'page' | 'pull' | 'boot' | 'wait';

/** A runner that could not run at all (as opposed to a run that failed its cases): it never started, or it was closed under the call. */
export class CodeRunnerError extends Error {
  constructor(
    public readonly kind: 'unavailable' | 'closed',
    message: string,
    public readonly stage?: CodeRunnerStage
  ) {
    super(message);
    this.name = 'CodeRunnerError';
  }
}

export interface CodeDraftPort {
  read(problemId: string, language: string): Promise<string | null>;
  write(problemId: string, language: string, code: string | null): Promise<void>;
}

export interface LeetCodePorts {
  libraryStorage: LibraryStoragePort;
  libraryContent: LibraryContentPort;
  libraryCover: LibraryCoverPort;
  /** A monotonic clock in milliseconds; the editor's cold start is measured with it. */
  clock: () => number;
  /** The CodeMirror page and the typed editor API over it. */
  editorBridge: EditorBridgePort;
  codeDrafts: CodeDraftPort;
  gateway: LeetCodeGatewayPort;
  voice: VoiceEditTransportPort;
  audio: AudioRecordingPort;
  /** The runner Run Local calls: it follows the setting. */
  runner: CodeRunnerPort;
  /** The on-device runner the setting can choose; a flow may supply its own. */
  deviceRunner: CodeRunnerPort;
  /** Config the container resolves once; it rides the ports object so a flow test can turn a flag on without a module mock. */
  flags: LeetCodeFlags;
}

/** What a caller may supply; the container fills the rest (the runner defaults to the gateway's). A test names only the flag it turns on. */
export type LeetCodePortOverrides = Partial<Omit<LeetCodePorts, 'flags'>> & {
  flags?: Partial<LeetCodeFlags>;
};

export class VoiceEditTransportError extends Error {
  constructor(
    public readonly status: number,
    message: string
  ) {
    super(message);
    this.name = 'VoiceEditTransportError';
  }
}
