// backend/remote-chat-agent-contracts/tests/providerRuntime.test.ts
//
// Round-trip + negative coverage for the discriminated
// `ProviderRuntimeEventSchema` and every payload schema it composes. One
// fixture per event type, one malformed object per payload family.
import { describe, expect, it } from 'vitest';
import { z } from 'zod';
import { ProviderRuntimeEventSchema } from '../src/contracts/providerRuntime.js';

const baseEventFields = {
  eventId: 'event-1',
  provider: 'opencode' as const,
  threadId: 'thread-1',
  createdAt: '2026-02-28T00:00:00.000Z',
};

const CREATED_AT = '2026-02-28T00:00:00.000Z';

type Fixture = z.input<typeof ProviderRuntimeEventSchema>;

const fixtures: Fixture[] = [
  {
    ...baseEventFields,
    type: 'session.started',
    payload: { message: 'session ready' },
  },
  {
    ...baseEventFields,
    eventId: 'event-2',
    type: 'session.state.changed',
    payload: { state: 'running', reason: 'turn-start' },
  },
  {
    ...baseEventFields,
    eventId: 'event-3',
    type: 'session.exited',
    payload: { reason: 'shutdown', recoverable: false, exitKind: 'graceful' },
  },
  {
    ...baseEventFields,
    eventId: 'event-4',
    type: 'thread.started',
    payload: { providerThreadId: 'provider-thread-1' },
  },
  {
    ...baseEventFields,
    eventId: 'event-5',
    type: 'thread.state.changed',
    payload: { state: 'idle' },
  },
  {
    ...baseEventFields,
    eventId: 'event-6',
    type: 'thread.metadata.updated',
    payload: { name: 'Thread 1', metadata: { project: 'rca' } },
  },
  {
    ...baseEventFields,
    eventId: 'event-7',
    type: 'thread.token-usage.updated',
    payload: { usage: { usedTokens: 31251, maxTokens: 200000 } },
  },
  {
    ...baseEventFields,
    eventId: 'event-8',
    turnId: 'turn-1',
    type: 'turn.started',
    payload: { model: 'claude-sonnet-4-5', effort: 'high' },
  },
  {
    ...baseEventFields,
    eventId: 'event-9',
    turnId: 'turn-1',
    type: 'turn.completed',
    payload: { state: 'completed', stopReason: 'end_turn' },
  },
  {
    ...baseEventFields,
    eventId: 'event-10',
    turnId: 'turn-1',
    type: 'turn.aborted',
    payload: { reason: 'user-cancel' },
  },
  {
    ...baseEventFields,
    eventId: 'event-11',
    turnId: 'turn-1',
    type: 'turn.plan.updated',
    payload: {
      explanation: 'Plan',
      plan: [
        { step: 'one', status: 'completed' },
        { step: 'two', status: 'inProgress' },
      ],
    },
  },
  {
    ...baseEventFields,
    eventId: 'event-12',
    turnId: 'turn-1',
    type: 'turn.proposed.delta',
    payload: { delta: '# Step' },
  },
  {
    ...baseEventFields,
    eventId: 'event-13',
    turnId: 'turn-1',
    type: 'turn.proposed.completed',
    payload: { planMarkdown: '# Ship it' },
  },
  {
    ...baseEventFields,
    eventId: 'event-14',
    turnId: 'turn-1',
    type: 'turn.diff.updated',
    payload: { unifiedDiff: '--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new' },
  },
  {
    ...baseEventFields,
    eventId: 'event-15',
    itemId: 'item-1',
    type: 'item.started',
    payload: { itemType: 'command_execution', agentId: 'agent-a', parentToolUseId: 'tu-1' },
  },
  {
    ...baseEventFields,
    eventId: 'event-16',
    itemId: 'item-1',
    type: 'item.updated',
    payload: { itemType: 'file_change', status: 'inProgress' },
  },
  {
    ...baseEventFields,
    eventId: 'event-17',
    itemId: 'item-1',
    type: 'item.completed',
    payload: { itemType: 'file_change', status: 'completed' },
  },
  {
    ...baseEventFields,
    eventId: 'event-18',
    itemId: 'item-1',
    type: 'content.delta',
    payload: { streamKind: 'assistant_text', delta: 'hi' },
  },
  {
    ...baseEventFields,
    eventId: 'event-19',
    requestId: 'request-1',
    type: 'request.opened',
    payload: { requestType: 'command_execution_approval', detail: 'run tests' },
  },
  {
    ...baseEventFields,
    eventId: 'event-20',
    requestId: 'request-1',
    type: 'request.resolved',
    payload: { requestType: 'command_execution_approval', decision: 'accept' },
  },
  {
    ...baseEventFields,
    eventId: 'event-21',
    requestId: 'request-2',
    type: 'user-input.requested',
    payload: {
      questions: [
        {
          id: 'sandbox_mode',
          header: 'Sandbox',
          question: 'Which mode?',
          options: [
            { label: 'workspace-write', description: 'Edits in workspace only' },
            { label: 'danger-full-access', description: 'Unrestricted' },
          ],
        },
      ],
    },
  },
  {
    ...baseEventFields,
    eventId: 'event-22',
    requestId: 'request-2',
    type: 'user-input.resolved',
    payload: { answers: { sandbox_mode: 'workspace-write' } },
  },
  {
    ...baseEventFields,
    eventId: 'event-23',
    type: 'tool.denied',
    payload: { toolName: 'bash', reason: 'permission' },
  },
  {
    ...baseEventFields,
    eventId: 'event-24',
    type: 'runtime.warning',
    payload: { message: 'flaky connection' },
  },
  {
    ...baseEventFields,
    eventId: 'event-25',
    type: 'runtime.error',
    payload: { message: 'boom', class: 'provider_error' },
  },
  {
    ...baseEventFields,
    eventId: 'event-26',
    type: 'model.rerouted',
    payload: { fromModel: 'gpt-4', toModel: 'gpt-5', reason: 'fallback' },
  },
];

describe('given ProviderRuntimeEventSchema', () => {
  it('when one fixture per event type is parsed, then it round-trips', () => {
    expect(fixtures).toHaveLength(26);

    for (const fixture of fixtures) {
      const parsed = ProviderRuntimeEventSchema.safeParse(fixture);
      expect(parsed.success, JSON.stringify({ fixture, error: parsed.error })).toBe(true);
      if (!parsed.success) continue;
      const reserialized = ProviderRuntimeEventSchema.parse(parsed.data);
      expect(reserialized).toEqual(parsed.data);
    }
  });

  it('when provider is outside the opencode|claude|codex enum, then it rejects', () => {
    expect(
      ProviderRuntimeEventSchema.safeParse({
        ...fixtures[0],
        eventId: 'event-bad-provider',
        provider: 'ollama',
      }).success,
    ).toBe(false);
  });

  it('when threadId is the empty string, then it rejects (branded ThreadId is .min(1))', () => {
    expect(
      ProviderRuntimeEventSchema.safeParse({
        ...fixtures[0],
        eventId: 'event-bad-thread',
        threadId: '',
      }).success,
    ).toBe(false);
  });

  it('when createdAt is not an ISO datetime, then it rejects', () => {
    expect(
      ProviderRuntimeEventSchema.safeParse({
        ...fixtures[0],
        eventId: 'event-bad-date',
        createdAt: 'not-a-date',
      }).success,
    ).toBe(false);
  });

  it('when an unknown event type is given, then the discriminated union rejects', () => {
    expect(
      ProviderRuntimeEventSchema.safeParse({
        ...baseEventFields,
        type: 'message.delta',
        payload: { delta: 'legacy' },
      }).success,
    ).toBe(false);
  });
});

describe('given a malformed object per payload family', () => {
  const baseFields = {
    eventId: 'event-bad',
    provider: 'claude' as const,
    threadId: 'thread-1',
    createdAt: CREATED_AT,
  };

  it('when a session.* event is missing its required field, then it rejects', () => {
    expect(
      ProviderRuntimeEventSchema.safeParse({
        ...baseFields,
        type: 'session.state.changed',
        payload: { reason: 'no-state' },
      }).success,
    ).toBe(false);
  });

  it('when a thread.* event is missing its required field, then it rejects', () => {
    expect(
      ProviderRuntimeEventSchema.safeParse({
        ...baseFields,
        type: 'thread.state.changed',
        payload: { detail: { reason: 'no-state' } },
      }).success,
    ).toBe(false);
  });

  it('when a turn.* event is missing its required field, then it rejects', () => {
    expect(
      ProviderRuntimeEventSchema.safeParse({
        ...baseFields,
        type: 'turn.completed',
        payload: { stopReason: 'no-state' },
      }).success,
    ).toBe(false);
  });

  it('when an item.* event is missing its required field, then it rejects', () => {
    expect(
      ProviderRuntimeEventSchema.safeParse({
        ...baseFields,
        type: 'item.started',
        payload: { status: 'inProgress' },
      }).success,
    ).toBe(false);
  });

  it('when a content.delta event is missing streamKind, then it rejects', () => {
    expect(
      ProviderRuntimeEventSchema.safeParse({
        ...baseFields,
        type: 'content.delta',
        payload: { delta: 'hi' },
      }).success,
    ).toBe(false);
  });

  it('when a request.* event is missing requestType, then it rejects', () => {
    expect(
      ProviderRuntimeEventSchema.safeParse({
        ...baseFields,
        type: 'request.opened',
        payload: { detail: 'no-type' },
      }).success,
    ).toBe(false);
  });

  it('when a user-input.* event is missing questions, then it rejects', () => {
    expect(
      ProviderRuntimeEventSchema.safeParse({
        ...baseFields,
        type: 'user-input.requested',
        payload: { answers: {} },
      }).success,
    ).toBe(false);
  });

  it('when a tool.denied event is missing toolName, then it rejects', () => {
    expect(
      ProviderRuntimeEventSchema.safeParse({
        ...baseFields,
        type: 'tool.denied',
        payload: { reason: 'no-name' },
      }).success,
    ).toBe(false);
  });

  it('when a runtime.* event is missing message, then it rejects', () => {
    expect(
      ProviderRuntimeEventSchema.safeParse({
        ...baseFields,
        type: 'runtime.error',
        payload: { detail: 'no-message' },
      }).success,
    ).toBe(false);
  });

  it('when a model.rerouted event is missing reason, then it rejects', () => {
    expect(
      ProviderRuntimeEventSchema.safeParse({
        ...baseFields,
        type: 'model.rerouted',
        payload: { fromModel: 'a', toModel: 'b' },
      }).success,
    ).toBe(false);
  });
});
