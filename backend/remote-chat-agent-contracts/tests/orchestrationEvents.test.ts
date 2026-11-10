// backend/remote-chat-agent-contracts/tests/orchestrationEvents.test.ts
//
// One hand-written fixture per OrchestrationEvent variant (17 in total).
// Each fixture parses, the parsed value re-parses to itself, and the
// discriminator (`type`) is preserved end-to-end. The negative case is the
// single contract-mandated one: a missing `sequence` must reject the event.
import { describe, expect, it } from 'vitest';
import { OrchestrationEventSchema } from '../src/contracts/orchestrationEvents.js';

const CREATED_AT = '2026-02-28T00:00:00.000Z';
const UPDATED_AT = '2026-02-28T00:01:00.000Z';

const base = {
  eventId: 'evt-1',
  sequence: 1,
  createdAt: CREATED_AT,
  threadId: 'thread-1',
  actor: 'client' as const,
};

const threadCreated = {
  ...base,
  type: 'thread.created' as const,
  payload: {
    title: 'Hello thread',
    cwd: '/repo',
    modelSelection: { cli: 'opencode' as const, providerID: 'anthropic', modelID: 'claude-sonnet-4-5' },
    runtimeMode: 'auto-accept-edits' as const,
    interactionMode: 'default' as const,
    createdAt: CREATED_AT,
  },
};

const threadDeleted = {
  ...base,
  type: 'thread.deleted' as const,
  payload: { deletedAt: CREATED_AT },
};

const threadArchived = {
  ...base,
  type: 'thread.archived' as const,
  payload: { archivedAt: CREATED_AT, updatedAt: UPDATED_AT },
};

const threadUnarchived = {
  ...base,
  type: 'thread.unarchived' as const,
  payload: { updatedAt: UPDATED_AT },
};

const threadMetaUpdated = {
  ...base,
  type: 'thread.meta-updated' as const,
  payload: { title: 'Renamed thread', updatedAt: UPDATED_AT },
};

const threadRuntimeModeSet = {
  ...base,
  type: 'thread.runtime-mode-set' as const,
  payload: { runtimeMode: 'auto' as const, updatedAt: UPDATED_AT },
};

const threadInteractionModeSet = {
  ...base,
  type: 'thread.interaction-mode-set' as const,
  payload: { interactionMode: 'plan' as const, updatedAt: UPDATED_AT },
};

const threadMessageSent = {
  ...base,
  type: 'thread.message-sent' as const,
  payload: {
    message: {
      id: 'msg-1',
      role: 'user' as const,
      text: 'hi',
      turnId: 'turn-1',
      createdAt: CREATED_AT,
      updatedAt: UPDATED_AT,
    },
  },
};

const threadTurnStartRequested = {
  ...base,
  type: 'thread.turn-start-requested' as const,
  payload: {
    messageId: 'msg-1',
    runtimeMode: 'auto-accept-edits' as const,
    interactionMode: 'default' as const,
    createdAt: CREATED_AT,
  },
};

const threadTurnInterruptRequested = {
  ...base,
  type: 'thread.turn-interrupt-requested' as const,
  payload: { turnId: 'turn-1', createdAt: CREATED_AT },
};

const threadApprovalResponseRequested = {
  ...base,
  type: 'thread.approval-response-requested' as const,
  payload: {
    requestId: 'req-1',
    decision: 'accept' as const,
    createdAt: CREATED_AT,
  },
};

const threadUserInputResponseRequested = {
  ...base,
  type: 'thread.user-input-response-requested' as const,
  payload: {
    requestId: 'req-2',
    answers: { q1: 'answer one' },
    createdAt: CREATED_AT,
  },
};

const threadSessionStopRequested = {
  ...base,
  type: 'thread.session-stop-requested' as const,
  payload: { createdAt: CREATED_AT },
};

const threadSessionSet = {
  ...base,
  type: 'thread.session-set' as const,
  payload: {
    session: {
      cli: 'opencode' as const,
      providerSessionId: 'sess-1',
      status: 'ready' as const,
      activeTurnId: 'turn-1',
    },
  },
};

const threadProposedPlanUpserted = {
  ...base,
  type: 'thread.proposed-plan-upserted' as const,
  payload: {
    proposedPlan: {
      id: 'plan-1',
      turnId: 'turn-1',
      planMarkdown: '# Plan\n- step one',
      createdAt: CREATED_AT,
      updatedAt: UPDATED_AT,
    },
  },
};

const threadActivityAppended = {
  ...base,
  type: 'thread.activity-appended' as const,
  payload: {
    activity: {
      id: 'act-1',
      tone: 'info' as const,
      kind: 'session.started',
      summary: 'session ready',
      payload: { provider: 'opencode' },
      turnId: 'turn-1',
      sequence: 1,
      createdAt: CREATED_AT,
    },
  },
};

const threadTurnDiffUpdated = {
  ...base,
  type: 'thread.turn-diff-updated' as const,
  payload: { turnId: 'turn-1', unifiedDiff: '--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new' },
};

const fixtures = [
  threadCreated,
  threadDeleted,
  threadArchived,
  threadUnarchived,
  threadMetaUpdated,
  threadRuntimeModeSet,
  threadInteractionModeSet,
  threadMessageSent,
  threadTurnStartRequested,
  threadTurnInterruptRequested,
  threadApprovalResponseRequested,
  threadUserInputResponseRequested,
  threadSessionStopRequested,
  threadSessionSet,
  threadProposedPlanUpserted,
  threadActivityAppended,
  threadTurnDiffUpdated,
] as const;

describe('given one fixture per OrchestrationEvent variant', () => {
  it('when each of 17 events is parsed, then it round-trips and `type` is preserved', () => {
    expect(fixtures).toHaveLength(17);
    for (const fixture of fixtures) {
      const parsed = OrchestrationEventSchema.parse(fixture);
      expect(parsed.type).toBe(fixture.type);
      expect(OrchestrationEventSchema.parse(parsed)).toEqual(parsed);
    }
  });
});

describe('given an OrchestrationEvent missing sequence', () => {
  it('when parsed, then it is rejected', () => {
    const broken = { ...threadCreated } as Record<string, unknown>;
    delete broken['sequence'];
    expect(OrchestrationEventSchema.safeParse(broken).success).toBe(false);
  });
});