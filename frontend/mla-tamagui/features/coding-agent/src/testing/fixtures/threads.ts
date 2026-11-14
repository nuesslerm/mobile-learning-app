// features/coding-agent/src/fixtures/threads.ts
//
// Deterministic `OrchestrationThread` fixtures that drive the coding-agent UI
// and the mock WS server. Every fixture satisfies `OrchestrationThreadSchema`
// (parsed at module load) so the wire shape is exercised at compile time and
// at runtime — this is the one place the contracts package imports a value
// from `@app/remote-chat-agent-contracts/orchestration`. ids are stable
// (`fx-…`) so tests, the impl, and the mocks package agree on which fixture
// is which.

import {
  ApprovalRequestId,
  EventId,
  MessageId,
  ThreadId,
  TurnId,
  OrchestrationThreadSchema,
  type OrchestrationThread,
} from '@app/remote-chat-agent-contracts/orchestration';

export const FIXTURE_IDS = [
  'fx-basic',
  'fx-streaming',
  'fx-thinking',
  'fx-tool-calls',
  'fx-diff',
  'fx-approval',
  'fx-user-input',
  'fx-plan',
  'fx-error',
  'fx-multi-turn',
] as const;
export type FixtureId = (typeof FIXTURE_IDS)[number];

const T_BASIC = ThreadId.parse('fx-thread-basic');
const T_STREAMING = ThreadId.parse('fx-thread-streaming');
const T_THINKING = ThreadId.parse('fx-thread-thinking');
const T_TOOL_CALLS = ThreadId.parse('fx-thread-tool-calls');
const T_DIFF = ThreadId.parse('fx-thread-diff');
const T_APPROVAL = ThreadId.parse('fx-thread-approval');
const T_USER_INPUT = ThreadId.parse('fx-thread-user-input');
const T_PLAN = ThreadId.parse('fx-thread-plan');
const T_ERROR = ThreadId.parse('fx-thread-error');
const T_MULTI_TURN = ThreadId.parse('fx-thread-multi-turn');

const TURN_BASIC = TurnId.parse('fx-turn-basic');
const TURN_STREAMING = TurnId.parse('fx-turn-streaming');
const TURN_THINKING = TurnId.parse('fx-turn-thinking');
const TURN_TOOL_CALLS = TurnId.parse('fx-turn-tool-calls');
const TURN_DIFF = TurnId.parse('fx-turn-diff');
const TURN_APPROVAL = TurnId.parse('fx-turn-approval');
const TURN_USER_INPUT = TurnId.parse('fx-turn-user-input');
const TURN_PLAN = TurnId.parse('fx-turn-plan');
const TURN_ERROR = TurnId.parse('fx-turn-error');
const TURN_MULTI_TURN_1 = TurnId.parse('fx-turn-multi-1');
const TURN_MULTI_TURN_2 = TurnId.parse('fx-turn-multi-2');
const TURN_MULTI_TURN_3 = TurnId.parse('fx-turn-multi-3');

const REQ_APPROVAL_COMMAND = ApprovalRequestId.parse('fx-req-approval-command');
const REQ_APPROVAL_FILE = ApprovalRequestId.parse('fx-req-approval-file');
const REQ_USER_INPUT = ApprovalRequestId.parse('fx-req-user-input');

const ISO_T0 = '2026-04-01T00:00:00.000Z';
const ISO_T1 = '2026-04-01T00:00:01.000Z';
const ISO_T2 = '2026-04-01T00:00:02.000Z';
const ISO_T3 = '2026-04-01T00:00:03.000Z';
const ISO_T4 = '2026-04-01T00:00:04.000Z';
const ISO_T5 = '2026-04-01T00:00:05.000Z';
const ISO_T6 = '2026-04-01T00:00:06.000Z';
const ISO_T7 = '2026-04-01T00:00:07.000Z';
const ISO_T8 = '2026-04-01T00:00:08.000Z';
const ISO_T9 = '2026-04-01T00:00:09.000Z';

const fxBasic = OrchestrationThreadSchema.parse({
  id: T_BASIC,
  title: 'Hello world',
  cwd: '/home/user/repo',
  modelSelection: { cli: 'opencode', modelID: 'gpt-5.4' },
  runtimeMode: 'full-access',
  interactionMode: 'default',
  latestTurn: {
    turnId: TURN_BASIC,
    state: 'completed',
    requestedAt: ISO_T0,
    startedAt: ISO_T0,
    completedAt: ISO_T1,
  },
  createdAt: ISO_T0,
  updatedAt: ISO_T1,
  archivedAt: null,
  deletedAt: null,
  messages: [
    {
      id: MessageId.parse('fx-basic-user-1'),
      role: 'user',
      text: 'Say hello in five words.',
      turnId: TURN_BASIC,
      createdAt: ISO_T0,
      updatedAt: ISO_T0,
    },
    {
      id: MessageId.parse('fx-basic-assistant-1'),
      role: 'assistant',
      text: 'Hello there, friend, greetings, hi.',
      turnId: TURN_BASIC,
      createdAt: ISO_T1,
      updatedAt: ISO_T1,
    },
  ],
  proposedPlans: [],
  activities: [],
  session: {
    cli: 'opencode',
    providerSessionId: 'fx-session-basic',
    status: 'ready',
    activeTurnId: undefined,
  },
});

const fxStreaming = OrchestrationThreadSchema.parse({
  id: T_STREAMING,
  title: 'Streaming assistant',
  cwd: '/home/user/repo',
  modelSelection: { cli: 'opencode', modelID: 'gpt-5.4' },
  runtimeMode: 'approval-required',
  interactionMode: 'default',
  latestTurn: {
    turnId: TURN_STREAMING,
    state: 'running',
    requestedAt: ISO_T0,
    startedAt: ISO_T0,
    completedAt: null,
  },
  createdAt: ISO_T0,
  updatedAt: ISO_T2,
  archivedAt: null,
  deletedAt: null,
  messages: [
    {
      id: MessageId.parse('fx-streaming-user-1'),
      role: 'user',
      text: 'Stream me a haiku about TypeScript.',
      turnId: TURN_STREAMING,
      createdAt: ISO_T0,
      updatedAt: ISO_T0,
    },
    {
      id: MessageId.parse('fx-streaming-assistant-1'),
      role: 'assistant',
      text: 'Types flow through the code,',
      turnId: TURN_STREAMING,
      createdAt: ISO_T1,
      updatedAt: ISO_T2,
    },
  ],
  proposedPlans: [],
  activities: [],
  session: {
    cli: 'opencode',
    providerSessionId: 'fx-session-streaming',
    status: 'running',
    activeTurnId: TURN_STREAMING,
  },
});

const fxThinking = OrchestrationThreadSchema.parse({
  id: T_THINKING,
  title: 'Reasoning turn',
  cwd: '/home/user/repo',
  modelSelection: { cli: 'opencode', modelID: 'gpt-5.4' },
  runtimeMode: 'approval-required',
  interactionMode: 'default',
  latestTurn: {
    turnId: TURN_THINKING,
    state: 'running',
    requestedAt: ISO_T0,
    startedAt: ISO_T0,
    completedAt: null,
  },
  createdAt: ISO_T0,
  updatedAt: ISO_T3,
  archivedAt: null,
  deletedAt: null,
  messages: [
    {
      id: MessageId.parse('fx-thinking-user-1'),
      role: 'user',
      text: 'Walk me through your approach.',
      turnId: TURN_THINKING,
      createdAt: ISO_T0,
      updatedAt: ISO_T0,
    },
    {
      id: MessageId.parse('fx-thinking-assistant-1'),
      role: 'assistant',
      text: 'Here is the plan I will follow.',
      turnId: TURN_THINKING,
      createdAt: ISO_T3,
      updatedAt: ISO_T3,
    },
  ],
  proposedPlans: [],
  activities: [
    {
      id: EventId.parse('fx-thinking-activity-1'),
      tone: 'info',
      kind: 'reasoning',
      summary: 'Reasoning',
      payload: {
        kind: 'reasoning_text',
        text: 'First I will inspect the input, then pick the smallest change.',
      },
      turnId: TURN_THINKING,
      sequence: 0,
      createdAt: ISO_T1,
    },
  ],
  session: {
    cli: 'opencode',
    providerSessionId: 'fx-session-thinking',
    status: 'running',
    activeTurnId: TURN_THINKING,
  },
});

const fxToolCalls = OrchestrationThreadSchema.parse({
  id: T_TOOL_CALLS,
  title: 'Heavy tool run',
  cwd: '/home/user/repo',
  modelSelection: { cli: 'opencode', modelID: 'gpt-5.4' },
  runtimeMode: 'approval-required',
  interactionMode: 'default',
  latestTurn: {
    turnId: TURN_TOOL_CALLS,
    state: 'running',
    requestedAt: ISO_T0,
    startedAt: ISO_T0,
    completedAt: null,
  },
  createdAt: ISO_T0,
  updatedAt: ISO_T9,
  archivedAt: null,
  deletedAt: null,
  messages: [
    {
      id: MessageId.parse('fx-tool-calls-user-1'),
      role: 'user',
      text: 'Run the test suite and patch the failure.',
      turnId: TURN_TOOL_CALLS,
      createdAt: ISO_T0,
      updatedAt: ISO_T0,
    },
    {
      id: MessageId.parse('fx-tool-calls-user-2'),
      role: 'user',
      text: 'Anything else?',
      createdAt: ISO_T9,
      updatedAt: ISO_T9,
    },
  ],
  proposedPlans: [],
  activities: [
    {
      id: EventId.parse('fx-tool-calls-activity-1'),
      tone: 'tool',
      kind: 'tool.completed',
      summary: 'pnpm install',
      payload: {
        itemType: 'command_execution',
        command: 'pnpm install',
        status: 'completed',
      },
      turnId: TURN_TOOL_CALLS,
      sequence: 0,
      createdAt: ISO_T1,
    },
    {
      id: EventId.parse('fx-tool-calls-activity-2'),
      tone: 'tool',
      kind: 'tool.completed',
      summary: 'pnpm typecheck',
      payload: {
        itemType: 'command_execution',
        command: 'pnpm typecheck',
        status: 'completed',
      },
      turnId: TURN_TOOL_CALLS,
      sequence: 1,
      createdAt: ISO_T2,
    },
    {
      id: EventId.parse('fx-tool-calls-activity-3'),
      tone: 'tool',
      kind: 'tool.completed',
      summary: 'pnpm lint',
      payload: {
        itemType: 'command_execution',
        command: 'pnpm lint',
        status: 'completed',
      },
      turnId: TURN_TOOL_CALLS,
      sequence: 2,
      createdAt: ISO_T3,
    },
    {
      id: EventId.parse('fx-tool-calls-activity-4'),
      tone: 'tool',
      kind: 'tool.completed',
      summary: 'pnpm test:unit',
      payload: {
        itemType: 'command_execution',
        command: 'pnpm test:unit',
        status: 'completed',
      },
      turnId: TURN_TOOL_CALLS,
      sequence: 3,
      createdAt: ISO_T4,
    },
    {
      id: EventId.parse('fx-tool-calls-activity-5'),
      tone: 'error',
      kind: 'tool.failed',
      summary: 'pnpm test:e2e',
      payload: {
        itemType: 'command_execution',
        command: 'pnpm test:e2e',
        status: 'failed',
      },
      turnId: TURN_TOOL_CALLS,
      sequence: 4,
      createdAt: ISO_T5,
    },
    {
      id: EventId.parse('fx-tool-calls-activity-6'),
      tone: 'error',
      kind: 'tool.declined',
      summary: 'pnpm publish',
      payload: {
        itemType: 'command_execution',
        command: 'pnpm publish',
        status: 'declined',
      },
      turnId: TURN_TOOL_CALLS,
      sequence: 5,
      createdAt: ISO_T6,
    },
    {
      id: EventId.parse('fx-tool-calls-activity-7'),
      tone: 'tool',
      kind: 'tool.completed',
      summary: 'Edit src/foo.ts',
      payload: {
        itemType: 'file_change',
        status: 'completed',
        data: { changedFiles: ['src/foo.ts'] },
      },
      turnId: TURN_TOOL_CALLS,
      sequence: 6,
      createdAt: ISO_T7,
    },
    {
      id: EventId.parse('fx-tool-calls-activity-8'),
      tone: 'tool',
      kind: 'tool.completed',
      summary: 'Edit src/foo.test.ts',
      payload: {
        itemType: 'file_change',
        status: 'completed',
        data: { changedFiles: ['src/foo.test.ts'] },
      },
      turnId: TURN_TOOL_CALLS,
      sequence: 7,
      createdAt: ISO_T8,
    },
  ],
  session: {
    cli: 'opencode',
    providerSessionId: 'fx-session-tool-calls',
    status: 'running',
    activeTurnId: TURN_TOOL_CALLS,
  },
});

const FX_DIFF_PAYLOAD = [
  'diff --git a/src/foo.ts b/src/foo.ts',
  'index 1111111..2222222 100644',
  '--- a/src/foo.ts',
  '+++ b/src/foo.ts',
  '@@ -1,3 +1,3 @@',
  '-export const a = 1;',
  '-export const b = 2;',
  '+export const a = 1;',
  '+export const b = 3;',
  ' diff --git a/src/bar.ts b/src/bar.ts',
  'index 3333333..4444444 100644',
  '--- a/src/bar.ts',
  '+++ b/src/bar.ts',
  '@@ -1,3 +1,3 @@',
  '-export const x = "x";',
  '-export const y = "y";',
  '+export const x = "x";',
  '+export const y = "z";',
  '',
].join('\n');

const fxDiff = OrchestrationThreadSchema.parse({
  id: T_DIFF,
  title: 'Turn with a unified diff',
  cwd: '/home/user/repo',
  modelSelection: { cli: 'opencode', modelID: 'gpt-5.4' },
  runtimeMode: 'auto-accept-edits',
  interactionMode: 'default',
  latestTurn: {
    turnId: TURN_DIFF,
    state: 'running',
    requestedAt: ISO_T0,
    startedAt: ISO_T0,
    completedAt: null,
  },
  createdAt: ISO_T0,
  updatedAt: ISO_T2,
  archivedAt: null,
  deletedAt: null,
  messages: [
    {
      id: MessageId.parse('fx-diff-user-1'),
      role: 'user',
      text: 'Refactor the constants in foo and bar.',
      turnId: TURN_DIFF,
      createdAt: ISO_T0,
      updatedAt: ISO_T0,
    },
  ],
  proposedPlans: [],
  activities: [
    {
      id: EventId.parse('fx-diff-activity-1'),
      tone: 'info',
      kind: 'turn.diff.updated',
      summary: 'Diff for current turn',
      payload: {
        turnId: TURN_DIFF,
        unifiedDiff: FX_DIFF_PAYLOAD,
      },
      turnId: TURN_DIFF,
      sequence: 0,
      createdAt: ISO_T1,
    },
  ],
  session: {
    cli: 'opencode',
    providerSessionId: 'fx-session-diff',
    status: 'running',
    activeTurnId: TURN_DIFF,
  },
});

const fxApproval = OrchestrationThreadSchema.parse({
  id: T_APPROVAL,
  title: 'Pending approvals',
  cwd: '/home/user/repo',
  modelSelection: { cli: 'opencode', modelID: 'gpt-5.4' },
  runtimeMode: 'approval-required',
  interactionMode: 'default',
  latestTurn: {
    turnId: TURN_APPROVAL,
    state: 'running',
    requestedAt: ISO_T0,
    startedAt: ISO_T0,
    completedAt: null,
  },
  createdAt: ISO_T0,
  updatedAt: ISO_T2,
  archivedAt: null,
  deletedAt: null,
  messages: [
    {
      id: MessageId.parse('fx-approval-user-1'),
      role: 'user',
      text: 'Run the deploy and patch the config.',
      turnId: TURN_APPROVAL,
      createdAt: ISO_T0,
      updatedAt: ISO_T0,
    },
  ],
  proposedPlans: [],
  activities: [
    {
      id: EventId.parse('fx-approval-activity-1'),
      tone: 'approval',
      kind: 'request.opened',
      summary: 'Approve command',
      payload: {
        requestId: REQ_APPROVAL_COMMAND,
        requestType: 'command_execution_approval',
        detail: 'pnpm deploy:prod',
      },
      turnId: TURN_APPROVAL,
      sequence: 0,
      createdAt: ISO_T1,
    },
    {
      id: EventId.parse('fx-approval-activity-2'),
      tone: 'approval',
      kind: 'request.opened',
      summary: 'Approve patch',
      payload: {
        requestId: REQ_APPROVAL_FILE,
        requestType: 'file_change_approval',
        detail: 'src/config/prod.ts',
      },
      turnId: TURN_APPROVAL,
      sequence: 1,
      createdAt: ISO_T2,
    },
  ],
  session: {
    cli: 'opencode',
    providerSessionId: 'fx-session-approval',
    status: 'ready',
    activeTurnId: TURN_APPROVAL,
  },
});

const fxUserInput = OrchestrationThreadSchema.parse({
  id: T_USER_INPUT,
  title: 'Need your input',
  cwd: '/home/user/repo',
  modelSelection: { cli: 'opencode', modelID: 'gpt-5.4' },
  runtimeMode: 'approval-required',
  interactionMode: 'default',
  latestTurn: {
    turnId: TURN_USER_INPUT,
    state: 'running',
    requestedAt: ISO_T0,
    startedAt: ISO_T0,
    completedAt: null,
  },
  createdAt: ISO_T0,
  updatedAt: ISO_T1,
  archivedAt: null,
  deletedAt: null,
  messages: [
    {
      id: MessageId.parse('fx-user-input-user-1'),
      role: 'user',
      text: 'Help me pick the next step.',
      turnId: TURN_USER_INPUT,
      createdAt: ISO_T0,
      updatedAt: ISO_T0,
    },
  ],
  proposedPlans: [],
  activities: [
    {
      id: EventId.parse('fx-user-input-activity-1'),
      tone: 'approval',
      kind: 'user-input.requested',
      summary: 'Need input',
      payload: {
        requestId: REQ_USER_INPUT,
        questions: [
          {
            id: 'q-auth',
            header: 'Auth',
            question: 'Which auth strategy should we use?',
            options: [
              { label: 'OAuth', description: 'Standard OAuth 2.0 flow.' },
              {
                label: 'API key',
                description: 'Long-lived API key in the header.',
              },
            ],
            multiSelect: false,
          },
          {
            id: 'q-scopes',
            header: 'Scopes',
            question: 'Which scopes should the token grant?',
            options: [
              { label: 'read', description: 'Read-only access.' },
              { label: 'write', description: 'Read and write.' },
              { label: 'admin', description: 'Full administrative access.' },
            ],
            multiSelect: true,
          },
        ],
      },
      turnId: TURN_USER_INPUT,
      sequence: 0,
      createdAt: ISO_T1,
    },
  ],
  session: {
    cli: 'opencode',
    providerSessionId: 'fx-session-user-input',
    status: 'ready',
    activeTurnId: TURN_USER_INPUT,
  },
});

const fxPlan = OrchestrationThreadSchema.parse({
  id: T_PLAN,
  title: 'Proposed plan',
  cwd: '/home/user/repo',
  modelSelection: { cli: 'opencode', modelID: 'gpt-5.4' },
  runtimeMode: 'approval-required',
  interactionMode: 'plan',
  latestTurn: {
    turnId: TURN_PLAN,
    state: 'running',
    requestedAt: ISO_T0,
    startedAt: ISO_T0,
    completedAt: null,
  },
  createdAt: ISO_T0,
  updatedAt: ISO_T1,
  archivedAt: null,
  deletedAt: null,
  messages: [
    {
      id: MessageId.parse('fx-plan-user-1'),
      role: 'user',
      text: 'Sketch a plan before you start coding.',
      turnId: TURN_PLAN,
      createdAt: ISO_T0,
      updatedAt: ISO_T0,
    },
  ],
  proposedPlans: [
    {
      id: 'fx-plan-1',
      turnId: TURN_PLAN,
      planMarkdown:
        '# Refactor plan\n\n' +
        '- [ ] Inspect the current module layout.\n' +
        '- [ ] Extract the shared helpers.\n' +
        '- [ ] Update the call sites.\n' +
        '- [ ] Add regression tests.\n',
      createdAt: ISO_T1,
      updatedAt: ISO_T1,
    },
  ],
  activities: [],
  session: {
    cli: 'opencode',
    providerSessionId: 'fx-session-plan',
    status: 'running',
    activeTurnId: TURN_PLAN,
  },
});

const fxError = OrchestrationThreadSchema.parse({
  id: T_ERROR,
  title: 'Turn failed',
  cwd: '/home/user/repo',
  modelSelection: { cli: 'opencode', modelID: 'gpt-5.4' },
  runtimeMode: 'auto-accept-edits',
  interactionMode: 'default',
  latestTurn: {
    turnId: TURN_ERROR,
    state: 'error',
    requestedAt: ISO_T0,
    startedAt: ISO_T0,
    completedAt: ISO_T2,
  },
  createdAt: ISO_T0,
  updatedAt: ISO_T2,
  archivedAt: null,
  deletedAt: null,
  messages: [
    {
      id: MessageId.parse('fx-error-user-1'),
      role: 'user',
      text: 'Build the release image.',
      turnId: TURN_ERROR,
      createdAt: ISO_T0,
      updatedAt: ISO_T0,
    },
  ],
  proposedPlans: [],
  activities: [
    {
      id: EventId.parse('fx-error-activity-1'),
      tone: 'error',
      kind: 'runtime.error',
      summary: 'Provider error',
      payload: {
        message: 'Provider returned 500 while streaming the response.',
        class: 'provider_error',
      },
      turnId: TURN_ERROR,
      sequence: 0,
      createdAt: ISO_T2,
    },
  ],
  session: {
    cli: 'opencode',
    providerSessionId: 'fx-session-error',
    status: 'error',
    activeTurnId: undefined,
    lastError: 'Provider returned 500 while streaming the response.',
  },
});

const fxMultiTurn = OrchestrationThreadSchema.parse({
  id: T_MULTI_TURN,
  title: 'Multi-turn conversation',
  cwd: '/home/user/repo',
  modelSelection: { cli: 'opencode', modelID: 'gpt-5.4' },
  runtimeMode: 'full-access',
  interactionMode: 'default',
  latestTurn: {
    turnId: TURN_MULTI_TURN_3,
    state: 'completed',
    requestedAt: ISO_T4,
    startedAt: ISO_T4,
    completedAt: ISO_T6,
  },
  createdAt: ISO_T0,
  updatedAt: ISO_T6,
  archivedAt: null,
  deletedAt: null,
  messages: [
    {
      id: MessageId.parse('fx-multi-turn-user-1'),
      role: 'user',
      text: 'What is the capital of France?',
      turnId: TURN_MULTI_TURN_1,
      createdAt: ISO_T0,
      updatedAt: ISO_T0,
    },
    {
      id: MessageId.parse('fx-multi-turn-assistant-1'),
      role: 'assistant',
      text: 'Paris.',
      turnId: TURN_MULTI_TURN_1,
      createdAt: ISO_T1,
      updatedAt: ISO_T1,
    },
    {
      id: MessageId.parse('fx-multi-turn-user-2'),
      role: 'user',
      text: 'And of Italy?',
      turnId: TURN_MULTI_TURN_2,
      createdAt: ISO_T2,
      updatedAt: ISO_T2,
    },
    {
      id: MessageId.parse('fx-multi-turn-assistant-2'),
      role: 'assistant',
      text: 'Rome.',
      turnId: TURN_MULTI_TURN_2,
      createdAt: ISO_T3,
      updatedAt: ISO_T3,
    },
    {
      id: MessageId.parse('fx-multi-turn-user-3'),
      role: 'user',
      text: 'And of Japan?',
      turnId: TURN_MULTI_TURN_3,
      createdAt: ISO_T4,
      updatedAt: ISO_T4,
    },
    {
      id: MessageId.parse('fx-multi-turn-assistant-3'),
      role: 'assistant',
      text: 'Tokyo.',
      turnId: TURN_MULTI_TURN_3,
      createdAt: ISO_T5,
      updatedAt: ISO_T6,
    },
  ],
  proposedPlans: [],
  activities: [],
  session: {
    cli: 'opencode',
    providerSessionId: 'fx-session-multi-turn',
    status: 'ready',
    activeTurnId: undefined,
  },
});

export const FIXTURE_THREADS: Record<FixtureId, OrchestrationThread> = {
  'fx-basic': fxBasic,
  'fx-streaming': fxStreaming,
  'fx-thinking': fxThinking,
  'fx-tool-calls': fxToolCalls,
  'fx-diff': fxDiff,
  'fx-approval': fxApproval,
  'fx-user-input': fxUserInput,
  'fx-plan': fxPlan,
  'fx-error': fxError,
  'fx-multi-turn': fxMultiTurn,
};
