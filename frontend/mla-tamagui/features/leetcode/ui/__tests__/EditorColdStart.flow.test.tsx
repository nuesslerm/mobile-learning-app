import { createFakeAudioRecorder, createFakeVoiceEditTransport } from '@lib/flow-test-harness';
import { describe, expect, it, vi } from 'vitest';

import { app } from '../../../../tests/flow/app';
import { forgetLoggedLines, loggedLines } from '../../../../tests/flow/loggedLines';
import type { LeetCodeGatewayPort, LeetCodePortOverrides, VoiceEditTransportPort } from '../..';

const FIELDS = ['nth', 'htmlBytes', 'evalMs', 'mountToReadyMs', 'readyToHydratedMs', 'totalMs'];

describe('the editor cold start', () => {
  it('given the editor opens on two-sum, when it hydrates, then one editor.cold_start line holds every duration as a finite non-negative number', async () => {
    // Given a screen opened with nothing logged yet
    await forgetLoggedLines();
    const gateway: LeetCodeGatewayPort = {
      userStatus: vi.fn(async () => ({ isSignedIn: true, username: 'flow-user' })),
      test: vi.fn(),
      submit: vi.fn(),
      poll: vi.fn(),
      runLocal: vi.fn(),
      format: vi.fn(async (p: { lang: string; code: string }) => p.code),
      loginCaptureStatus: vi.fn(async () => ({ active: false, expiresAt: null })),
    };
    const ports: LeetCodePortOverrides = {
      gateway,
      voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort,
      audio: createFakeAudioRecorder(),
    };

    // When the editor hydrates
    const session = await app.render('/leetcode/two-sum', { overrides: { leetcode: ports } });
    await session.editor();

    // Then exactly one line was logged, and each of its fields is finite and not negative
    const lines = await loggedLines('editor.cold_start');
    expect(lines).toHaveLength(1);
    for (const name of FIELDS) {
      const value = (lines[0]!.context as Record<string, unknown>)[name];
      expect(typeof value, name).toBe('number');
      expect(Number.isFinite(value as number), name).toBe(true);
      expect(value as number, name).toBeGreaterThanOrEqual(0);
    }
  });
});
