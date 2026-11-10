// backend/remote-chat-agent-contracts/tests/unit/contracts.test.ts
//
// Schema round-trip tests for the contracts in `src/contracts/*` that carry
// real branching (discriminated unions, enums, regexes, required-vs-optional
// fields). Plain shape-only schemas with no branching (e.g. HealthResponse)
// are not tested here: a passing safeParse on a schema with no
// union/enum/regex tests nothing beyond "this is an object".
import { describe, it, expect } from 'vitest';
import {
  ControlMessageRequestSchema,
  ControlMessageResponseSchema,
} from '../../src/contracts/control.js';
import { HealthResponseSchema } from '../../src/contracts/health.js';
import { McpActionResponseSchema, McpServerMapSchema } from '../../src/contracts/mcp.js';
import { RecentModelsResponseSchema } from '../../src/contracts/models.js';

describe('given control contracts', () => {
  it('when a queue/steer request/response pair is parsed, then both accept', () => {
    expect(
      ControlMessageRequestSchema.safeParse({ sessionId: 's1', directory: '/d', message: 'm' }).success,
    ).toBe(true);
    expect(
      ControlMessageResponseSchema.safeParse({ ok: true, action: 'queue', sessionId: 's1', message: 'm' }).success,
    ).toBe(true);
  });

  it('when action is not one of queue/steer, then the response rejects', () => {
    expect(
      ControlMessageResponseSchema.safeParse({ ok: true, action: 'stop', sessionId: 's1', message: 'm' }).success,
    ).toBe(false);
  });
});

describe('given the remaining proxy-route contracts', () => {
  it('when a health response is parsed, then it accepts', () => {
    expect(HealthResponseSchema.safeParse({ status: 'ok' }).success).toBe(true);
  });

  it('when a health response has the wrong status literal, then it rejects', () => {
    expect(HealthResponseSchema.safeParse({ status: 'down' }).success).toBe(false);
  });

  it('when an mcp server map is parsed, then it accepts the statuses opencode declares and rejects the ones it does not', () => {
    expect(
      McpServerMapSchema.safeParse({
        filesystem: { status: 'connected' },
        off: { status: 'disabled' },
        github: { status: 'needs_auth' },
        broken: { status: 'failed', error: 'oops' },
        fresh: { status: 'needs_client_registration', error: 'register it' },
      }).success,
    ).toBe(true);
    expect(McpServerMapSchema.safeParse({ broken: { status: 'failed' } }).success).toBe(false);
    expect(McpServerMapSchema.safeParse({ fresh: { status: 'needs_client_registration' } }).success).toBe(false);
    expect(McpServerMapSchema.safeParse({ x: { status: 'exploding' } }).success).toBe(false);
  });

  it('when a connect or disconnect reply is parsed, then it is the boolean the service wraps and nothing else', () => {
    expect(McpActionResponseSchema.safeParse({ ok: true }).success).toBe(true);
    expect(McpActionResponseSchema.safeParse({ ok: false }).success).toBe(true);
    expect(McpActionResponseSchema.safeParse({ status: 'connected' }).success).toBe(false);
  });

  it('when a recent-models response is parsed, then it requires a count', () => {
    expect(
      RecentModelsResponseSchema.safeParse({ data: [{ id: 'a', providerID: 'anthropic', name: 'A' }] }).success,
    ).toBe(false);
    expect(
      RecentModelsResponseSchema.safeParse({ data: [{ id: 'a', providerID: 'anthropic', name: 'A', count: 3 }] })
        .success,
    ).toBe(true);
  });
});
