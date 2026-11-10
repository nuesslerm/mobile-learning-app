import {
  createFakeAudioRecorder,
  createFakeVoiceEditTransport,
  screen,
  userEvent,
  waitFor,
} from '@lib/flow-test-harness';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import {
  usePlanStore,
  type LeetCodeGatewayPort,
  type LeetCodePorts,
  type RunResponse,
  type VoiceEditTransportPort,
} from '../../features/leetcode';

import { app } from './app';

// Native modules ProblemEditorScreen (mounted beside the plan tab) pulls in
// that cannot load under jsdom; they are never exercised here. The harness has
// no stubs for them, so they stay mocked.



const sendVoicePlan = vi.fn(async (_body: unknown) => ({
  transcript: 'plan',
  wallMs: 21,
  alternatives: [
    {
      id: 'alt-1',
      label: 'Answer',
      summary: 's',
      confidence: 0.9,
      commands: [
        {
          type: 'replaceLines',
          startLine: 3,
          endLine: 3,
          newText: '        return s == s[::-1]',
        },
      ],
    },
  ],
}));
const sendEditStats = vi.fn(async (_row: unknown) => undefined);
const sendVoiceTranscribe = vi.fn(async () => ({ transcript: '', wallMs: 1 }));

const ACCEPTED_RUN: RunResponse = {
  source: 'backend-run',
  kind: 'test',
  state: 'SUCCESS',
  requestId: 'flow-request',
  run_success: true,
  status_msg: 'Accepted',
  total_correct: 5,
  total_testcases: 5,
};

function fakeGateway(): LeetCodeGatewayPort {
  return {
    userStatus: vi.fn(async () => ({ isSignedIn: true, username: 'flow-user' })),
    test: vi.fn(async () => ({ ...ACCEPTED_RUN, interpret_id: 'flow-interpret' })),
    submit: vi.fn(async () => ACCEPTED_RUN),
    poll: vi.fn(async () => ACCEPTED_RUN),
    runLocal: vi.fn(async () => ACCEPTED_RUN),
    format: vi.fn(async (p: { lang: string; code: string }) => `# formatted\n${p.code}`),
    loginCaptureStatus: vi.fn(async () => ({ active: false, expiresAt: null })),
  };
}

const PROBLEM = 'valid-palindrome';

async function renderPlan() {
  const ports: LeetCodePorts = {
    gateway: fakeGateway(),
    voice: {
      ...createFakeVoiceEditTransport(),
      plan: (body: unknown) => sendVoicePlan(body),
      transcribe: () => sendVoiceTranscribe(),
      sendEditStats: (row: unknown) => sendEditStats(row),
    } as unknown as VoiceEditTransportPort,
    audio: createFakeAudioRecorder(),
  };
  await app.render(`/leetcode/${PROBLEM}`, { overrides: { leetcode: ports } });
  await userEvent.click(screen.getByTestId('ProblemWorkspaceTab-plan'));
}

async function addBlock(text: string) {
  const input = screen.getByTestId('ProblemPlanTextInput');
  await userEvent.type(input, text);
  await userEvent.click(screen.getByTestId('ProblemPlanAddButton'));
}

describe('PlanTab flow', () => {
  beforeEach(() => {
    usePlanStore.setState({ plans: {} });
    sendVoicePlan.mockClear();
    sendEditStats.mockClear();
    sendVoiceTranscribe.mockClear();
  });

  it('given a typed two-sentence block, when Add is pressed, then it lands as one step until Split is pressed', async () => {
    await renderPlan();

    await addBlock(
      'Keep only alphanumeric characters. Compare the cleaned string with its reverse.'
    );

    await waitFor(() => {
      expect(screen.getAllByTestId(/^ProblemPlanStep-\d+$/)).toHaveLength(1);
    });
    expect(screen.getByTestId('ProblemPlanStep-0-Text').textContent).toContain(
      'Keep only alphanumeric characters'
    );

    await userEvent.click(screen.getByTestId('ProblemPlanStep-0-Split'));

    await waitFor(() => {
      expect(screen.getAllByTestId(/^ProblemPlanStep-\d+$/)).toHaveLength(2);
    });
    expect(screen.getByTestId('ProblemPlanStep-1-Text').textContent).toContain(
      'Compare the cleaned string'
    );
  });

  it('given two plan steps, when Delete is pressed on the second row, then one row remains', async () => {
    await renderPlan();
    await addBlock('Alpha first');
    await addBlock('Beta second');

    await waitFor(() => {
      expect(screen.getAllByTestId(/^ProblemPlanStep-\d+$/)).toHaveLength(2);
    });
    await userEvent.click(screen.getByTestId('ProblemPlanStep-1-Delete'));

    await waitFor(() => {
      expect(screen.getAllByTestId(/^ProblemPlanStep-\d+$/)).toHaveLength(1);
    });
  });

  it('given a pending step, when Realize is pressed, then sendVoicePlan runs once and the row shows its realized lines', async () => {
    await renderPlan();
    await addBlock('Return whether the cleaned string is a palindrome.');

    await waitFor(() => {
      expect(screen.getAllByTestId(/^ProblemPlanStep-\d+$/)).toHaveLength(1);
    });
    await userEvent.click(screen.getByTestId('ProblemPlanStep-0-Realize'));

    await waitFor(() => {
      expect(sendVoicePlan).toHaveBeenCalledTimes(1);
      expect(
        screen.getByTestId('ProblemPlanStep-0-Status').textContent
      ).toMatch(/lines \d+\u2013\d+/);
    });
  });
});