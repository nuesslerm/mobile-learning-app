// frontend/libraries/remote-chat-agent-contracts/tests/orchestrationCore.test.ts
//
// One hand-written fixture per schema: a thread with two messages, three
// activities (info/tool/approval tones), a session, and a proposed plan. Every
// schema must parse its fixture and the parsed value must re-parse to itself.
// The only negative case here is the one the contract calls out explicitly:
// an unknown `runtimeMode` must fail the closed enum.
import { describe, expect, it } from 'vitest';
import {
  OrchestrationLatestTurnSchema,
  OrchestrationMessageSchema,
  OrchestrationProposedPlanSchema,
  OrchestrationReadModelSchema,
  OrchestrationSessionSchema,
  OrchestrationThreadActivitySchema,
  OrchestrationThreadSchema,
} from '../src/contracts/orchestrationCore.js';

const CREATED_AT = '2026-02-28T00:00:00.000Z';
const UPDATED_AT = '2026-02-28T00:01:00.000Z';
const REQUESTED_AT = '2026-02-28T00:00:01.000Z';
const STARTED_AT = '2026-02-28T00:00:02.000Z';
const COMPLETED_AT = '2026-02-28T00:00:30.000Z';

const threadFixture = {
  id: 'thread-1',
  title: 'Hello thread',
  cwd: '/repo',
  modelSelection: {
    cli: 'opencode' as const,
    providerID: 'anthropic',
    modelID: 'claude-sonnet-4-5',
  },
  runtimeMode: 'auto-accept-edits' as const,
  interactionMode: 'default' as const,
  latestTurn: {
    turnId: 'turn-1',
    state: 'completed' as const,
    requestedAt: REQUESTED_AT,
    startedAt: STARTED_AT,
    completedAt: COMPLETED_AT,
  },
  createdAt: CREATED_AT,
  updatedAt: UPDATED_AT,
  messages: [
    {
      id: 'msg-1',
      role: 'user' as const,
      text: 'hi',
      turnId: 'turn-1',
      createdAt: CREATED_AT,
      updatedAt: UPDATED_AT,
    },
    {
      id: 'msg-2',
      role: 'assistant' as const,
      text: 'hello there',
      turnId: 'turn-1',
      createdAt: CREATED_AT,
      updatedAt: UPDATED_AT,
    },
  ],
  proposedPlans: [
    {
      id: 'plan-1',
      turnId: 'turn-1',
      planMarkdown: '# Plan\n- step one',
      createdAt: CREATED_AT,
      updatedAt: UPDATED_AT,
    },
  ],
  activities: [
    {
      id: 'act-1',
      tone: 'info' as const,
      kind: 'session.started',
      summary: 'session ready',
      payload: { provider: 'opencode' },
      turnId: 'turn-1',
      sequence: 1,
      createdAt: CREATED_AT,
    },
    {
      id: 'act-2',
      tone: 'tool' as const,
      kind: 'tool.denied',
      summary: 'tool was denied',
      payload: { toolName: 'bash' },
      turnId: 'turn-1',
      sequence: 2,
      createdAt: CREATED_AT,
    },
    {
      id: 'act-3',
      tone: 'approval' as const,
      kind: 'request.opened',
      summary: 'approval requested',
      payload: { requestType: 'command_execution_approval' },
      turnId: 'turn-1',
      sequence: 3,
      createdAt: CREATED_AT,
    },
  ],
  session: {
    cli: 'opencode' as const,
    providerSessionId: 'sess-1',
    status: 'ready' as const,
    activeTurnId: 'turn-1',
  },
};

const readModelFixture = {
  snapshotSequence: 42,
  threads: { 'thread-1': threadFixture },
  updatedAt: UPDATED_AT,
};

describe('given OrchestrationThreadSchema', () => {
  it('when a thread with 2 messages, 3 activities (info/tool/approval), a session, and a proposed plan is parsed, then it round-trips', () => {
    const parsed = OrchestrationThreadSchema.parse(threadFixture);
    expect(OrchestrationThreadSchema.parse(parsed)).toEqual(parsed);
    expect(parsed.messages).toHaveLength(2);
    expect(parsed.activities.map((a) => a.tone)).toEqual(['info', 'tool', 'approval']);
    expect(parsed.proposedPlans).toHaveLength(1);
    expect(parsed.session?.cli).toBe('opencode');
  });

  it('when runtimeMode is outside the closed enum, then parse fails', () => {
    expect(
      OrchestrationThreadSchema.safeParse({ ...threadFixture, runtimeMode: 'yolo' }).success,
    ).toBe(false);
  });
});

describe('given one fixture per remaining schema', () => {
  it('when each is parsed, then it round-trips', () => {
    const thread = OrchestrationThreadSchema.parse(threadFixture);
    const session = OrchestrationSessionSchema.parse(thread.session);
    const plan = OrchestrationProposedPlanSchema.parse(thread.proposedPlans[0]);
    const turn = OrchestrationLatestTurnSchema.parse(thread.latestTurn);
    const activity = OrchestrationThreadActivitySchema.parse(thread.activities[0]);
    const message = OrchestrationMessageSchema.parse(thread.messages[0]);
    const readModel = OrchestrationReadModelSchema.parse(readModelFixture);

    expect(OrchestrationSessionSchema.parse(session)).toEqual(session);
    expect(OrchestrationProposedPlanSchema.parse(plan)).toEqual(plan);
    expect(OrchestrationLatestTurnSchema.parse(turn)).toEqual(turn);
    expect(OrchestrationThreadActivitySchema.parse(activity)).toEqual(activity);
    expect(OrchestrationMessageSchema.parse(message)).toEqual(message);
    expect(OrchestrationReadModelSchema.parse(readModel)).toEqual(readModel);
    expect(Object.keys(readModel.threads)).toEqual(['thread-1']);
  });
});
