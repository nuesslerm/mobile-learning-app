import {
  routerHarness,
  screen,
  userEvent,
  waitFor,
} from '@lib/flow-test-harness';
import { describe, expect, it } from 'vitest';

import {
  createFixtureReportingGateway,
  createHttpReportingGateway,
  type ReportingFixtures,
} from '../../features/dashboards/data/reportingGateway';

import { app } from './app';

const GiB = 1024 ** 3;

const readyFixtures: ReportingFixtures = {
  metricsCurrent: {
    samples: [
      { ts: 1_757_600_000, scope: 'host', name: 'host', metric: 'cpu', value: 40 },
      {
        ts: 1_757_600_000,
        scope: 'host',
        name: 'host',
        metric: 'memory_used',
        value: 3.5 * GiB,
      },
      {
        ts: 1_757_600_000,
        scope: 'container',
        name: 'vm-gateway',
        metric: 'cpu',
        value: 12,
      },
      {
        ts: 1_757_600_000,
        scope: 'container',
        name: 'vm-gateway',
        metric: 'memory',
        value: 0.5 * GiB,
      },
    ],
  },
  metricsSeries: {
    window: '24h',
    points: [
      { ts: 1_757_600_000, scope: 'host', name: 'host', metric: 'cpu', value: 40 },
      {
        ts: 1_757_600_000,
        scope: 'host',
        name: 'host',
        metric: 'memory_used',
        value: 3.5 * GiB,
      },
      {
        ts: 1_757_600_000,
        scope: 'container',
        name: 'vm-gateway',
        metric: 'cpu',
        value: 12,
      },
      {
        ts: 1_757_600_000,
        scope: 'container',
        name: 'vm-gateway',
        metric: 'memory',
        value: 0.5 * GiB,
      },
    ],
  },
  headroom: {
    resources: [
      { resource: 'cpu', used: 0.5, ceiling: 2, remaining: 1.5, unit: 'ocpu' },
      {
        resource: 'memory',
        used: 4 * GiB,
        ceiling: 12 * GiB,
        remaining: 8 * GiB,
        unit: 'bytes',
      },
    ],
  },
  plans: {
    plans: [
      {
        slug: 'coding-agent',
        units: 3,
        runIds: 6,
        resolved: 4,
        unresolved: 2,
        unpricedRuns: 0,
        days: [
          {
            day: '09-12',
            runs: 4,
            tokens: 120_000,
            cost: 1.2,
            notionalCost: 2.4,
            unpricedRuns: 0,
          },
        ],
      },
      {
        slug: 'settings-screen',
        units: 2,
        runIds: 3,
        resolved: 3,
        unresolved: 0,
        unpricedRuns: 0,
        days: [
          {
            day: '09-12',
            runs: 3,
            tokens: 40_000,
            cost: 0.4,
            notionalCost: 0.8,
            unpricedRuns: 0,
          },
        ],
      },
    ],
  },
  planDetail: {
    slug: 'coding-agent',
    startedAt: '2026-09-12T08:00:00Z',
    finishedAt: null,
    units: [
      {
        id: 'a1',
        title: 'The gateway to the reporting service',
        tier: 'unit',
        status: 'done',
        seconds: 120,
        opencodeUsd: 0.5,
        opencodeTokens: 20_000,
        runIds: ['run-001', 'run-002', 'run-003'],
        resolved: 1,
        unresolved: 2,
        runs: [
          {
            runId: 'run-001',
            model: 'claude-haiku-4-5',
            pool: 'openrouter',
            cost: 0.2,
            tokens: 8_000,
            durationMs: 60_000,
            day: '09-12',
            priced: true,
            notionalCost: 0.4,
          },
        ],
      },
      {
        id: 'a2',
        title: 'The plan bars join the unresolved count',
        tier: 'unit',
        status: 'done',
        seconds: null,
        opencodeUsd: null,
        opencodeTokens: null,
        runIds: ['run-004'],
        resolved: 1,
        unresolved: 0,
        runs: [
          {
            runId: 'run-004',
            model: 'claude-sonnet-5',
            pool: 'openrouter',
            cost: 0.8,
            tokens: 12_000,
            durationMs: 120_000,
            day: '09-12',
            priced: true,
            notionalCost: 1.6,
          },
        ],
      },
    ],
    runIds: 4,
    resolved: 2,
    unresolved: 2,
    unpricedRuns: 0,
    days: [
      {
        day: '09-12',
        runs: 2,
        tokens: 20_000,
        cost: 1.0,
        notionalCost: 2.0,
        unpricedRuns: 0,
      },
    ],
  },
  run: {
    runId: 'run-001',
    ts: '2026-09-12T08:30:00Z',
    day: '09-12',
    model: 'claude-haiku-4-5',
    pool: 'openrouter',
    cost: 0.2,
    tokens: 8_000,
    durationMs: 60_000,
    priced: true,
    notionalCost: 0.4,
  },
};

const readyGateway = createFixtureReportingGateway(readyFixtures);
const notConfiguredGateway = createHttpReportingGateway(undefined);

describe('Agent dashboards flow (fixture gateway, no network)', () => {
  it('given a ready fixture with two plans on one day, when the agents dashboard renders, then the overview shows one segment per plan for that day', async () => {
    await app.render('/dashboards/agents', {
      overrides: { dashboards: { reportingGateway: readyGateway } },
    });

    await waitFor(() => {
      expect(screen.getByTestId('AgentsOverviewChart-Bar-0-Label')).toBeTruthy();
    });

    expect(
      screen.getByTestId('AgentsOverviewChart-Bar-0-Label').textContent
    ).toBe('09-12');
    expect(
      screen.getByTestId('AgentsOverviewChart-Bar-0-Segment-0').textContent
    ).toContain('coding-agent');
    expect(
      screen.getByTestId('AgentsOverviewChart-Bar-0-Segment-1').textContent
    ).toContain('settings-screen');
    expect(
      screen.queryByTestId('AgentsOverviewChart-Bar-0-Segment-2')
    ).toBeNull();
    expect(
      screen.getByTestId('AgentsDashboardScreen-OverviewSummary').textContent
    ).toBe('2 plans · 7 of 9 runIds joined to a run record');
  });

  it('given a plan segment pressed at the overview, when the chart is tapped, then the view descends to that plan’s units (asserted on the rendered unit titles, not on a testID existing)', async () => {
    await app.render('/dashboards/agents', {
      overrides: { dashboards: { reportingGateway: readyGateway } },
    });

    await waitFor(() => {
      expect(
        screen.getByTestId('AgentsOverviewChart-Bar-0-Segment-0')
      ).toBeTruthy();
    });

    await userEvent.click(
      screen.getByTestId('AgentsOverviewChart-Bar-0-Segment-0')
    );

    await waitFor(() => {
      expect(
        screen.getByText('The gateway to the reporting service')
      ).toBeTruthy();
    });

    expect(
      screen.getByText('The plan bars join the unresolved count')
    ).toBeTruthy();
    expect(screen.getByTestId('AgentsDetailTitle').textContent).toContain(
      'units'
    );
    expect(screen.getByText('09-12 · coding-agent')).toBeTruthy();
  });

  it('given a unit with unresolved runIds at the plan level, when its bar renders, then the unresolved count is shown', async () => {
    await app.render('/dashboards/agents', {
      overrides: { dashboards: { reportingGateway: readyGateway } },
    });

    await waitFor(() => {
      expect(
        screen.getByTestId('AgentsOverviewChart-Bar-0-Segment-0')
      ).toBeTruthy();
    });

    await userEvent.click(
      screen.getByTestId('AgentsOverviewChart-Bar-0-Segment-0')
    );

    await waitFor(() => {
      expect(screen.getByText('2 unresolved')).toBeTruthy();
    });
  });

  it('given the drill-down has reached the leaf unit chart, when a run segment is pressed, then navigation was requested for the run route', async () => {
    const router = routerHarness();
    await app.render('/dashboards/agents', {
      overrides: { dashboards: { reportingGateway: readyGateway } },
    });

    await waitFor(() => {
      expect(
        screen.getByTestId('AgentsOverviewChart-Bar-0-Segment-0')
      ).toBeTruthy();
    });
    await userEvent.click(
      screen.getByTestId('AgentsOverviewChart-Bar-0-Segment-0')
    );

    await waitFor(() => {
      expect(
        screen.getByTestId('AgentsPlanChart-Bar-0-Segment-0')
      ).toBeTruthy();
    });
    await userEvent.click(
      screen.getByTestId('AgentsPlanChart-Bar-0-Segment-0')
    );

    await waitFor(() => {
      expect(
        screen.getByTestId('AgentsUnitChart-Bar-0-Segment-0')
      ).toBeTruthy();
    });
    await userEvent.click(
      screen.getByTestId('AgentsUnitChart-Bar-0-Segment-0')
    );

    expect(router.lastPush()).toBe('/coding-agent/runs/run-001');
  });

  it('given a not-configured gateway, when each dashboard renders, then it states the reason and draws no chart', async () => {
    await app.render('/dashboards/agents', {
      overrides: { dashboards: { reportingGateway: notConfiguredGateway } },
    });
    await waitFor(() => {
      expect(
        screen.getByTestId('AgentsDashboardScreen-NotConfigured')
      ).toBeTruthy();
    });
    expect(
      screen.getByTestId('AgentsDashboardScreen-NotConfigured').textContent
    ).toContain('Reporting service is not configured');
    expect(screen.queryByTestId('AgentsOverviewChart')).toBeNull();

    await app.render('/dashboards/infra', {
      overrides: { dashboards: { reportingGateway: notConfiguredGateway } },
    });
    await waitFor(() => {
      expect(
        screen.getByTestId('InfraDashboardScreen-NotConfigured')
      ).toBeTruthy();
    });
    expect(
      screen.getByTestId('InfraDashboardScreen-NotConfigured').textContent
    ).toContain('Reporting service is not configured');
    expect(screen.queryByTestId('InfraHostCpuChart')).toBeNull();
  });
});