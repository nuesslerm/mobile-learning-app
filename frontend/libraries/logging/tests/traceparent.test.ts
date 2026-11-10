import { describe, it, expect } from 'vitest';

import {
  allowedTraceparentOrigins,
  withTraceparent,
  type ActiveSpanLike,
} from '../src/primitives/traceparent.js';

const AGENT_ORIGIN = 'https://agent.example.tailnet.ts.net';
const LEETCODE_ORIGIN = 'http://localhost:8789';
const THIRD_PARTY_ORIGIN = 'https://analytics.example.com';

const activeSpan: ActiveSpanLike = {
  spanContext: () => ({
    traceId: '4bf92f3577b34da6a3ce929d0e0e4736',
    spanId: '00f067aa0ba902b7',
    traceFlags: 1,
  }),
};

describe('given allowedTraceparentOrigins', () => {
  describe('when passed configured base URLs', () => {
    it('then it derives the set of origins, ignoring null/undefined/malformed entries', () => {
      const origins = allowedTraceparentOrigins([
        `${AGENT_ORIGIN}/api`,
        LEETCODE_ORIGIN,
        null,
        undefined,
        'not a url',
      ]);
      expect(origins).toEqual(new Set([AGENT_ORIGIN, LEETCODE_ORIGIN]));
    });
  });
});

describe('given withTraceparent', () => {
  const allowedOrigins = allowedTraceparentOrigins([
    AGENT_ORIGIN,
    LEETCODE_ORIGIN,
  ]);

  describe('when there is no active span', () => {
    it('then it returns init unchanged, with no traceparent header', () => {
      const init = { method: 'POST', headers: { 'Content-Type': 'application/json' } };
      const result = withTraceparent(
        `${AGENT_ORIGIN}/api/convert`,
        init,
        null,
        allowedOrigins
      );
      expect(result).toBe(init);
      expect(result?.headers).not.toHaveProperty('traceparent');
    });
  });

  describe('when the destination origin is allowlisted', () => {
    it('then it attaches a valid W3C traceparent header matching the active span', () => {
      const result = withTraceparent(
        `${AGENT_ORIGIN}/api/convert`,
        { method: 'POST' },
        activeSpan,
        allowedOrigins
      );
      expect(result?.headers?.traceparent).toBe(
        '00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01'
      );
    });

    it('then it preserves existing headers alongside the new traceparent header', () => {
      const result = withTraceparent(
        `${LEETCODE_ORIGIN}/api/leetcode/run`,
        { headers: { 'Content-Type': 'application/json' } },
        activeSpan,
        allowedOrigins
      );
      expect(result?.headers).toEqual({
        'Content-Type': 'application/json',
        traceparent: '00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01',
      });
    });
  });

  describe('when the destination origin is not allowlisted (third-party)', () => {
    it('then it sends no traceparent header, even with an active span', () => {
      const result = withTraceparent(
        `${THIRD_PARTY_ORIGIN}/collect`,
        { method: 'POST' },
        activeSpan,
        allowedOrigins
      );
      expect(result?.headers).toBeUndefined();
    });
  });

  describe('when the destination is a relative URL', () => {
    it('then it sends no traceparent header (never guessed as same-origin)', () => {
      const result = withTraceparent(
        '/api/code-assist',
        { method: 'POST' },
        activeSpan,
        allowedOrigins
      );
      expect(result?.headers).toBeUndefined();
    });
  });
});
