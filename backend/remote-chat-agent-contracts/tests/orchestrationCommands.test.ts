// backend/remote-chat-agent-contracts/tests/orchestrationCommands.test.ts
//
// One hand-written fixture per ClientOrchestrationCommand variant (12 in
// total) plus one per stream item kind. Each fixture parses, the parsed value
// re-parses to itself, and the discriminator is preserved end-to-end. The
// negative case is the contract-mandated one: `thread.turn.start` without
// `runtimeMode` must reject.
import { describe, expect, it } from 'vitest';
import {
  ClientOrchestrationCommandSchema,
  OrchestrationCommandReceiptSchema,
  OrchestrationShellStreamItemSchema,
  OrchestrationThreadStreamItemSchema,
} from '../src/contracts/orchestrationCommands.js';

const CREATED_AT = '2026-02-28T00:00:00.000Z';
const UPDATED_AT = '2026-02-28T00:01:00.000Z';

const base = {
  commandId: 'cmd-1',
  threadId: 'thread-1',
};

const modelSelection = {
  cli: 'opencode' as const,
  providerID: 'anthropic',
  modelID: 'claude-sonnet-4-5',
};

const threadCreate = {
  ...base,
  type: 'thread.create' as const,
  title: 'Hello thread',
  cwd: '/repo',
  modelSelection,
  runtimeMode: 'approval-required' as const,
  interactionMode: 'default' as const,
  branch: 'feature/x',
  worktreePath: '/repo/.worktrees/feature-x',
  createdAt: CREATED_AT,
};

const threadDelete = { ...base, type: 'thread.delete' as const };

const threadArchive = { ...base, type: 'thread.archive' as const };

const threadUnarchive = { ...base, type: 'thread.unarchive' as const };

const threadMetaUpdate = {
  ...base,
  type: 'thread.meta.update' as const,
  title: 'Renamed thread',
  modelSelection,
  cwd: '/repo',
};

const threadRuntimeModeSet = {
  ...base,
  type: 'thread.runtime-mode.set' as const,
  runtimeMode: 'auto' as const,
  createdAt: CREATED_AT,
};

const threadInteractionModeSet = {
  ...base,
  type: 'thread.interaction-mode.set' as const,
  interactionMode: 'plan' as const,
  createdAt: CREATED_AT,
};

const threadTurnStart = {
  ...base,
  type: 'thread.turn.start' as const,
  message: {
    messageId: 'msg-1',
    role: 'user' as const,
    text: 'ship it',
    attachments: [{ kind: 'image', url: 'https://example.test/a.png' }],
  },
  modelSelection,
  runtimeMode: 'auto-accept-edits' as const,
  interactionMode: 'default' as const,
  sourceProposedPlan: { planId: 'plan-1' },
  bootstrap: {
    createThread: {
      title: 'Bootstrapped thread',
      cwd: '/repo',
      modelSelection,
      runtimeMode: 'full-access' as const,
      interactionMode: 'plan' as const,
    },
  },
  createdAt: CREATED_AT,
};

const threadTurnInterrupt = {
  ...base,
  type: 'thread.turn.interrupt' as const,
  turnId: 'turn-1',
  createdAt: CREATED_AT,
};

const threadApprovalRespond = {
  ...base,
  type: 'thread.approval.respond' as const,
  requestId: 'req-1',
  decision: 'acceptForSession' as const,
  createdAt: CREATED_AT,
};

const threadUserInputRespond = {
  ...base,
  type: 'thread.user-input.respond' as const,
  requestId: 'req-2',
  answers: { q1: 'answer one', q2: 42 },
  createdAt: CREATED_AT,
};

const threadSessionStop = {
  ...base,
  type: 'thread.session.stop' as const,
  createdAt: CREATED_AT,
};

const commandFixtures = [
  threadCreate,
  threadDelete,
  threadArchive,
  threadUnarchive,
  threadMetaUpdate,
  threadRuntimeModeSet,
  threadInteractionModeSet,
  threadTurnStart,
  threadTurnInterrupt,
  threadApprovalRespond,
  threadUserInputRespond,
  threadSessionStop,
] as const;

const threadSummary = {
  id: 'thread-1',
  title: 'Hello thread',
  cwd: '/repo',
  modelSelection,
  runtimeMode: 'approval-required' as const,
  interactionMode: 'default' as const,
  latestTurn: null,
  createdAt: CREATED_AT,
  updatedAt: UPDATED_AT,
  session: null,
};

const thread = {
  ...threadSummary,
  messages: [],
  proposedPlans: [],
  activities: [],
};

const event = {
  eventId: 'evt-1',
  sequence: 1,
  createdAt: CREATED_AT,
  threadId: 'thread-1',
  commandId: 'cmd-1',
  actor: 'client' as const,
  type: 'thread.archived' as const,
  payload: { archivedAt: CREATED_AT, updatedAt: UPDATED_AT },
};

describe('given one fixture per ClientOrchestrationCommand variant', () => {
  it('when each of 12 commands is parsed, then it round-trips and `type` is preserved', () => {
    expect(commandFixtures).toHaveLength(12);
    for (const fixture of commandFixtures) {
      const parsed = ClientOrchestrationCommandSchema.parse(fixture);
      expect(parsed.type).toBe(fixture.type);
      expect(parsed.commandId).toBe(fixture.commandId);
      expect(ClientOrchestrationCommandSchema.parse(parsed)).toEqual(parsed);
    }
  });
});

describe('given one fixture per orchestration thread stream item kind', () => {
  it('when each kind is parsed, then it round-trips', () => {
    const items = [
      { kind: 'synchronized' as const },
      { kind: 'snapshot' as const, snapshot: thread },
      { kind: 'event' as const, event },
    ] as const;
    expect(items).toHaveLength(3);
    for (const item of items) {
      const parsed = OrchestrationThreadStreamItemSchema.parse(item);
      expect(parsed.kind).toBe(item.kind);
      expect(OrchestrationThreadStreamItemSchema.parse(parsed)).toEqual(parsed);
    }
  });
});

describe('given one fixture per orchestration shell stream item kind', () => {
  it('when each kind is parsed, then it round-trips with a summary snapshot', () => {
    const items = [
      { kind: 'synchronized' as const },
      { kind: 'snapshot' as const, snapshot: [threadSummary] },
      { kind: 'event' as const, event },
    ] as const;
    expect(items).toHaveLength(3);
    for (const item of items) {
      const parsed = OrchestrationShellStreamItemSchema.parse(item);
      expect(parsed.kind).toBe(item.kind);
      expect(OrchestrationShellStreamItemSchema.parse(parsed)).toEqual(parsed);
    }
  });
});

describe('given an accepted and a rejected command receipt', () => {
  it('when parsed, then both round-trip', () => {
    const accepted = { commandId: 'cmd-1', status: 'accepted' as const, events: [event] };
    const rejected = {
      commandId: 'cmd-2',
      status: 'rejected' as const,
      events: [],
      error: { code: 'thread-not-found', message: 'no such thread' },
    };
    for (const receipt of [accepted, rejected]) {
      const parsed = OrchestrationCommandReceiptSchema.parse(receipt);
      expect(OrchestrationCommandReceiptSchema.parse(parsed)).toEqual(parsed);
    }
  });
});

describe('given a thread.turn.start command without runtimeMode', () => {
  it('when parsed, then it is rejected', () => {
    const broken = { ...threadTurnStart } as Record<string, unknown>;
    delete broken['runtimeMode'];
    expect(ClientOrchestrationCommandSchema.safeParse(broken).success).toBe(false);
  });
});
