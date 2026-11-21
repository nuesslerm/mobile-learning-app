import { screen, waitFor } from '@lib/flow-test-harness';
import { describe, expect, it } from 'vitest';

import { createFixturePorts as createCodingAgentPorts } from '@feature/coding-agent/testing';

import { app } from './app';

const ROUTES = [
  '/',
  '/leetcode/login',
  '/tamil',
  '/tamil/tracks',
  '/tamil/review',
  '/tamil/write/uyir-a',
  '/dashboards',
  '/dashboards/infra',
  '/dashboards/agents',
  '/coding-agent',
  '/coding-agent/fx-thread-fx-basic',
  '/coding-agent/runs/fx-run-finished',
  '/settings/appearance',
  '/service-report',
];

describe('feature routes mount under their providers', () => {
  it.each(ROUTES)('given the route %s, when the whole app tree renders it, then no screen throws for a missing provider', async (path) => {
    await app.render(path, { overrides: { 'coding-agent': createCodingAgentPorts() } });

    await waitFor(() => expect(screen.queryByText(/Provider missing/i)).toBeNull());
    expect(document.body.textContent?.length ?? 0).toBeGreaterThan(0);
  });
});
