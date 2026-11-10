// backend/remote-chat-agent-contracts/tests/wsRpc.test.ts
//
// Round-trip + negative coverage for the WS envelope and the
// method/subscription tables. One fixture per ClientEnvelope kind and per
// ServerEnvelope kind round-trips through their discriminated unions. The
// three contract-mandated edge cases:
//   1. A `request` envelope naming a method the WS_METHODS table does not
//      know about still parses (method is `z.string()`; the table is a
//      lookup helper, not a closed enum).
//   2. A malformed `response` envelope (here, `ok: true` paired with an
//      `error` field) is rejected by the schema's refine.
//   3. A wrong-shape `complete` envelope (missing `id`) is rejected by the
//      object schema before the discriminator even runs.
import { describe, expect, it } from 'vitest';
import { z } from 'zod';
import {
  ClientEnvelopeSchema,
  ServerEnvelopeSchema,
  ServerProbeResultSchema,
  WS_HEARTBEAT_INTERVAL_MS,
  WS_METHODS,
  WS_PATH,
  WS_PROTOCOL_VERSION,
  WS_SUBSCRIPTIONS,
  WsMethodSchema,
  WsSubscriptionSchema,
} from '../src/contracts/wsRpc.js';

const CREATED_AT = '2026-02-28T00:00:00.000Z';

const modelSelection = {
  cli: 'opencode' as const,
  providerID: 'anthropic',
  modelID: 'claude-sonnet-4-5',
};

const baseClientRequest = {
  kind: 'request' as const,
  id: 'req-1',
  method: 'orchestration.listThreads',
  params: { includeArchived: true },
};

const baseClientSubscribe = {
  kind: 'subscribe' as const,
  id: 'sub-1',
  method: 'orchestration.subscribeThread',
  params: { threadId: 'thread-1', afterSequence: 3 },
};

const baseClientUnsubscribe = { kind: 'unsubscribe' as const, id: 'sub-1' };

const baseClientPing = { kind: 'ping' as const, ts: 1700000000000 };

const clientEnvelopes = [
  baseClientRequest,
  baseClientSubscribe,
  baseClientUnsubscribe,
  baseClientPing,
] as const;

const baseServerHello = {
  kind: 'hello' as const,
  serverVersion: 'rca-2026.02.28',
  protocolVersion: WS_PROTOCOL_VERSION,
  heartbeatIntervalMs: WS_HEARTBEAT_INTERVAL_MS,
};

const baseServerResponseOk = {
  kind: 'response' as const,
  id: 'req-1',
  ok: true as const,
  result: {
    threads: [
      {
        id: 'thread-1',
        title: 'Hello thread',
        cwd: '/repo',
        modelSelection,
        runtimeMode: 'auto-accept-edits' as const,
        interactionMode: 'default' as const,
        latestTurn: null,
        createdAt: CREATED_AT,
        updatedAt: CREATED_AT,
        session: null,
      },
    ],
  },
};

const baseServerResponseErr = {
  kind: 'response' as const,
  id: 'req-2',
  ok: false as const,
  error: { code: 'thread-not-found', message: 'no such thread', retryable: false },
};

const baseServerEvent = {
  kind: 'event' as const,
  id: 'sub-1',
  item: {
    kind: 'event' as const,
    event: {
      eventId: 'evt-1',
      sequence: 1,
      createdAt: CREATED_AT,
      threadId: 'thread-1',
      commandId: 'cmd-1',
      actor: 'client' as const,
      type: 'thread.archived' as const,
      payload: { archivedAt: CREATED_AT, updatedAt: CREATED_AT },
    },
  },
};

const baseServerComplete = { kind: 'complete' as const, id: 'sub-1' };

const baseServerPong = { kind: 'pong' as const, ts: 1700000000000 };

const serverEnvelopes = [
  baseServerHello,
  baseServerResponseOk,
  baseServerResponseErr,
  baseServerEvent,
  baseServerComplete,
  baseServerPong,
] as const;

describe('given one ClientEnvelope fixture per kind', () => {
  it('when each of 4 envelopes is parsed, then it round-trips and `kind` is preserved', () => {
    expect(clientEnvelopes).toHaveLength(4);
    for (const envelope of clientEnvelopes) {
      const parsed = ClientEnvelopeSchema.parse(envelope);
      expect(parsed.kind).toBe(envelope.kind);
      expect(ClientEnvelopeSchema.parse(parsed)).toEqual(parsed);
    }
  });
});

describe('given one ServerEnvelope fixture per kind', () => {
  it('when each of 6 envelopes is parsed, then it round-trips and `kind` is preserved', () => {
    expect(serverEnvelopes).toHaveLength(6);
    for (const envelope of serverEnvelopes) {
      const parsed = ServerEnvelopeSchema.parse(envelope);
      expect(parsed.kind).toBe(envelope.kind);
      expect(ServerEnvelopeSchema.parse(parsed)).toEqual(parsed);
    }
  });
});

describe('given a request envelope naming a method that is not in WS_METHODS', () => {
  it('when parsed, then the envelope still parses and the table lookup is undefined', () => {
    const envelope = {
      ...baseClientRequest,
      method: 'orchestration.thisIsNotARealMethod',
      params: { anything: 'goes' },
    };
    const parsed = ClientEnvelopeSchema.parse(envelope);
    expect(parsed.kind).toBe('request');
    if (parsed.kind !== 'request') throw new Error('expected request envelope');
    expect(parsed.method).toBe('orchestration.thisIsNotARealMethod');
    expect(WS_METHODS[parsed.method as keyof typeof WS_METHODS]).toBeUndefined();
  });
});

describe('given a malformed response envelope', () => {
  it('when ok:true carries an `error` field, then parsing fails (refine rejects the shape)', () => {
    const broken = {
      ...baseServerResponseOk,
      error: { code: 'should-not-be-here', message: 'parser must reject this' },
    };
    const result = ServerEnvelopeSchema.safeParse(broken);
    expect(result.success).toBe(false);
  });

  it('when ok:false omits the `error` field, then parsing fails', () => {
    const broken = { kind: 'response' as const, id: 'req-3', ok: false as const };
    const result = ServerEnvelopeSchema.safeParse(broken);
    expect(result.success).toBe(false);
  });

  it('when ok:true omits the `result` field, then parsing fails', () => {
    const broken = { kind: 'response' as const, id: 'req-4', ok: true as const };
    const result = ServerEnvelopeSchema.safeParse(broken);
    expect(result.success).toBe(false);
  });
});

describe('given a complete envelope missing the `id` field', () => {
  it('when parsed, then it is rejected (object schema fails before discriminator)', () => {
    const broken = { kind: 'complete' };
    expect(ServerEnvelopeSchema.safeParse(broken).success).toBe(false);
  });
});

describe('given ServerProbeResultSchema', () => {
  it('when a complete probe result is parsed, then it round-trips', () => {
    const result = {
      availableClis: ['opencode' as const, 'claude' as const],
      selectedCli: 'opencode' as const,
      version: 'rca-2026.02.28',
      providers: [
        {
          cli: 'opencode' as const,
          displayName: 'OpenCode',
          available: true,
          models: [
            {
              providerID: 'anthropic',
              modelID: 'claude-sonnet-4-5',
              name: 'Claude Sonnet 4.5',
              isDefault: true,
            },
          ],
        },
      ],
    };
    const parsed = ServerProbeResultSchema.parse(result);
    expect(ServerProbeResultSchema.parse(parsed)).toEqual(parsed);
    expect(parsed.providers[0]?.models[0]?.isDefault).toBe(true);
  });

  it('when selectedCli is null, then it parses (no selected CLI yet)', () => {
    const result = ServerProbeResultSchema.parse({
      availableClis: [],
      selectedCli: null,
      version: 'rca-2026.02.28',
      providers: [],
    });
    expect(result.selectedCli).toBeNull();
  });
});

describe('given the WS RPC tables and constants', () => {
  it('when WS_METHODS is enumerated, then every method has a params and a result schema', () => {
    const methods = Object.keys(WS_METHODS) as Array<keyof typeof WS_METHODS>;
    expect(methods).toEqual([
      'orchestration.dispatchCommand',
      'orchestration.listThreads',
      'orchestration.getThread',
      'orchestration.getTurnDiff',
      'server.probe',
      'models.list',
    ]);
    for (const method of methods) {
      const entry = WS_METHODS[method];
      expect(entry.params).toBeInstanceOf(z.ZodType);
      expect(entry.result).toBeInstanceOf(z.ZodType);
    }
  });

  it('when WS_SUBSCRIPTIONS is enumerated, then every subscription has params and item schemas', () => {
    const subscriptions = Object.keys(WS_SUBSCRIPTIONS) as Array<
      keyof typeof WS_SUBSCRIPTIONS
    >;
    expect(subscriptions).toEqual([
      'orchestration.subscribeShell',
      'orchestration.subscribeThread',
    ]);
    for (const subscription of subscriptions) {
      const entry = WS_SUBSCRIPTIONS[subscription];
      expect(entry.params).toBeInstanceOf(z.ZodType);
      expect(entry.item).toBeInstanceOf(z.ZodType);
    }
  });

  it('when the protocol constants are read, then they match the contract', () => {
    expect(WS_PROTOCOL_VERSION).toBe(1);
    expect(WS_HEARTBEAT_INTERVAL_MS).toBe(15000);
    expect(WS_PATH).toBe('/ws');
  });

  it('when WsMethodSchema and WsSubscriptionSchema are applied, then every key in the tables parses', () => {
    for (const method of Object.keys(WS_METHODS)) {
      expect(WsMethodSchema.parse(method)).toBe(method);
    }
    for (const subscription of Object.keys(WS_SUBSCRIPTIONS)) {
      expect(WsSubscriptionSchema.parse(subscription)).toBe(subscription);
    }
  });
});
