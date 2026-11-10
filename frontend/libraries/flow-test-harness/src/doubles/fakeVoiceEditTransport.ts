// Structural mirror of features/leetcode/contracts/ports.ts `VoiceEditTransportPort` —
// the double stays import-free of mla-tamagui, resolves scripted responses with
// `wallMs: 0` and never touches the network.
type FakeEditCommand = { type: string; [key: string]: unknown };

type FakeEditAlternative = {
  id: string;
  label: string;
  summary: string;
  confidence: number;
  valid?: boolean;
  invalidReason?: string;
  commands: FakeEditCommand[];
};

type FakeVoiceTranscribeRequest = {
  audioUri: string;
  mimeType: string;
  filename: string;
  chunkIndex?: number;
};

type FakeVoicePlanRequest = {
  transcript: string;
  currentCode: string;
  language: string;
  source?: string;
  editId?: string;
};

type FakeClientEditStat = Record<string, unknown>;

export type FakeVoiceEditConfig = {
  transcript?: string;
  alternatives?: FakeEditAlternative[];
  /** When set, transcribe and plan reject with this error; sendEditStats still resolves (best-effort). */
  failWith?: Error;
};

export type FakeVoiceEditTransport = {
  transcribe(
    request: FakeVoiceTranscribeRequest & { editId?: string }
  ): Promise<{ transcript: string; wallMs: number }>;
  plan(
    request: FakeVoicePlanRequest
  ): Promise<{ transcript: string; alternatives: FakeEditAlternative[]; wallMs: number }>;
  sendEditStats(stat: FakeClientEditStat): Promise<void>;
  /** Method names called, oldest first. */
  calls: string[];
};

export function createFakeVoiceEditTransport(
  config: FakeVoiceEditConfig = {}
): FakeVoiceEditTransport {
  const transcript = config.transcript ?? '';
  const alternatives = config.alternatives ?? [];
  const failWith = config.failWith;
  const calls: string[] = [];

  return {
    async transcribe(): Promise<{ transcript: string; wallMs: number }> {
      calls.push('transcribe');
      if (failWith) throw failWith;
      return { transcript, wallMs: 0 };
    },
    async plan(): Promise<{
      transcript: string;
      alternatives: FakeEditAlternative[];
      wallMs: number;
    }> {
      calls.push('plan');
      if (failWith) throw failWith;
      return { transcript, alternatives, wallMs: 0 };
    },
    async sendEditStats(): Promise<void> {
      calls.push('sendEditStats');
    },
    get calls(): string[] {
      return [...calls];
    },
  };
}