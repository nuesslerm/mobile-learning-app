import { waitFor } from '@lib/flow-test-harness';
import { act } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { app } from '../../__tests__/flowApp';
import { ServiceReportContainer } from '../../di/ServiceReportContainer';
import type { GoldenCommentsPort } from '../../domain/ports';
import { createFixturePorts } from '../../testing';
import { useGoldenScreenshotComments } from '../useGoldenScreenshotComments';

function render(goldenComments: GoldenCommentsPort, screenshotKey: string | undefined) {
  return app.renderHook(() => useGoldenScreenshotComments(screenshotKey), {
    overrides: { 'service-report': { ...createFixturePorts(), goldenComments } },
    wrapper: ServiceReportContainer,
  });
}

describe('golden screenshot comments flow (port override)', () => {
  it('given a capture with one stored comment, when the hook mounts, then the thread is ready with that comment', async () => {
    const stored = { id: 1, screenshotKey: 'tamil/final', commentText: 'baseline looks right', createdAt: '2026-11-01T10:00:00.000Z' };
    const goldenComments: GoldenCommentsPort = {
      fetch: vi.fn(async () => ({ status: 'ready' as const, comments: [stored] })),
      post: vi.fn(async () => ({ status: 'ok' as const })),
    };

    const { result } = render(goldenComments, 'tamil/final');

    await waitFor(() => expect(result.current.status).toBe('ready'));
    expect(goldenComments.fetch).toHaveBeenCalledWith('tamil/final');
    expect(result.current.status === 'ready' && result.current.comments).toEqual([stored]);
  });

  it('given a ready thread, when a comment is posted, then the port gets it with the capture key and the thread reloads', async () => {
    const goldenComments: GoldenCommentsPort = {
      fetch: vi.fn(async () => ({ status: 'ready' as const, comments: [] })),
      post: vi.fn(async () => ({ status: 'ok' as const })),
    };
    const { result } = render(goldenComments, 'tamil/final');
    await waitFor(() => expect(result.current.status).toBe('ready'));

    let outcome: { ok: boolean } | undefined;
    await act(async () => {
      outcome = await result.current.post({ author: 'markus', commentText: 'baseline looks right' });
    });

    expect(outcome).toEqual({ ok: true });
    expect(goldenComments.post).toHaveBeenCalledWith({ screenshotKey: 'tamil/final', author: 'markus', commentText: 'baseline looks right' });
    await waitFor(() => expect(goldenComments.fetch).toHaveBeenCalledTimes(2));
  });

  it('given a port that is not configured, when a comment is posted, then the reason comes back and nothing reloads', async () => {
    const goldenComments: GoldenCommentsPort = {
      fetch: vi.fn(async () => ({ status: 'not-configured' as const, reason: 'ORDS is not set' })),
      post: vi.fn(async () => ({ status: 'not-configured' as const, reason: 'ORDS is not set' })),
    };
    const { result } = render(goldenComments, 'tamil/final');
    await waitFor(() => expect(result.current.status).toBe('not-configured'));

    let outcome: { ok: boolean; error?: string } | undefined;
    await act(async () => {
      outcome = await result.current.post({ commentText: 'hello' });
    });

    expect(outcome).toEqual({ ok: false, error: 'ORDS is not set' });
    expect(goldenComments.fetch).toHaveBeenCalledTimes(1);
  });
});
