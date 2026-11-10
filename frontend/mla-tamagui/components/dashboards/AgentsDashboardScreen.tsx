import { Logger } from '@lib/logging';
import {
  Card,
  CardContent,
  CardHeader,
  ScreenHeader,
  SegmentedControl,
  Spinner,
  StackedBarChart,
  Text,
  View,
  type StackedBarChartBar,
  type StackedBarChartColor,
  type StackedBarChartSegment,
} from '@lib/tamagui-design-system';
import { appConfig } from '@lib/app-config';
import { useDependencyOverrides } from '@lib/app-di';
import { backTo } from '@feature/settings';
import { router } from 'expo-router';
import { Fragment, useEffect, useMemo, useState } from 'react';
import { ScrollView } from 'react-native';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';

import {
  createHttpReportingGateway,
  type ReportingGateway,
  type ReportingResult,
} from './data/reportingGateway';
import type {
  PlanDayTotal,
  PlanDetail,
  PlansResponse,
  PlanSummary,
  ResolvedRun,
} from './data/reportingSchema';

type Measure = 'tokens' | 'cost' | 'notional';

const MEASURE_OPTIONS: { value: Measure; label: string }[] = [
  { value: 'tokens', label: 'Tokens' },
  { value: 'cost', label: 'Cost' },
  { value: 'notional', label: 'Notional' },
];

const MEASURE_LABEL: Record<Measure, string> = {
  tokens: 'Tokens',
  cost: 'Cost',
  notional: 'Notional cost',
};

const CHART_COLORS: StackedBarChartColor[] = [
  'primary',
  'accent',
  'success',
  'warning',
  'destructive',
  'muted',
];

// The segment the chart hands back on press; the extra fields carry the
// identity the drill-down and the run leaf need without leaking into the
// legend key (where the unpriced suffix lives).
type AgentSegment = StackedBarChartSegment & {
  slug?: string;
  unitId?: string;
  runId?: string;
};

type PlansState = { status: 'loading' } | ReportingResult<PlansResponse>;
type DetailState = { status: 'loading' } | ReportingResult<PlanDetail>;

type DrillState =
  | { level: 'overview' }
  | { level: 'plan'; day: string; slug: string }
  | { level: 'unit'; day: string; slug: string; unitId: string };

type Crumb = { label: string; onPress?: () => void };

function colorFor(index: number): StackedBarChartColor {
  return CHART_COLORS[index % CHART_COLORS.length];
}

function shortId(runId: string): string {
  return runId.length > 8 ? runId.slice(0, 8) : runId;
}

function keyFor(id: string, unpriced: boolean): string {
  return unpriced ? `${id} · unpriced` : id;
}

function dayValue(
  total: PlanDayTotal,
  measure: Measure,
): { value: number; unpriced: boolean } {
  if (measure === 'notional' && total.notionalCost === null) {
    return { value: 0, unpriced: true };
  }
  switch (measure) {
    case 'tokens':
      return { value: total.tokens, unpriced: false };
    case 'cost':
      return { value: total.cost, unpriced: false };
    case 'notional':
      return { value: total.notionalCost ?? 0, unpriced: false };
  }
}

function runValue(
  run: ResolvedRun,
  measure: Measure,
): { value: number; unpriced: boolean } {
  if (measure === 'notional' && run.notionalCost === null) {
    return { value: 0, unpriced: true };
  }
  switch (measure) {
    case 'tokens':
      return { value: run.tokens, unpriced: false };
    case 'cost':
      return { value: run.cost, unpriced: false };
    case 'notional':
      return { value: run.notionalCost ?? 0, unpriced: false };
  }
}

function buildOverviewBars(
  plans: PlansResponse,
  measure: Measure,
): StackedBarChartBar[] {
  const slugIndex = new Map<string, number>();
  plans.plans.forEach((plan, index) => slugIndex.set(plan.slug, index));
  const byDay = new Map<string, { plan: PlanSummary; total: PlanDayTotal }[]>();
  for (const plan of plans.plans) {
    for (const total of plan.days) {
      const entries = byDay.get(total.day) ?? [];
      entries.push({ plan, total });
      byDay.set(total.day, entries);
    }
  }
  return [...byDay.keys()].sort().map((day) => {
    const entries = byDay.get(day) ?? [];
    return {
      label: day,
      segments: entries.map(({ plan, total }) => {
        const value = dayValue(total, measure);
        const segment: AgentSegment = {
          key: keyFor(plan.slug, value.unpriced),
          value: value.value,
          color: colorFor(slugIndex.get(plan.slug) ?? 0),
          slug: plan.slug,
        };
        return segment;
      }),
    };
  });
}

function buildPlanBars(detail: PlanDetail, measure: Measure): StackedBarChartBar[] {
  return detail.units.map((unit) => {
    const segments: AgentSegment[] = unit.runs.map((run, index) => {
      const value = runValue(run, measure);
      return {
        key: keyFor(shortId(run.runId), value.unpriced),
        value: value.value,
        color: colorFor(index),
        unitId: unit.id,
        runId: run.runId,
      };
    });
    if (unit.unresolved > 0) {
      segments.push({
        key: `${unit.unresolved} unresolved`,
        value: 0,
        color: 'muted',
        unitId: unit.id,
      });
    }
    return { label: unit.title, segments };
  });
}

function buildUnitBars(
  detail: PlanDetail,
  unitId: string,
  measure: Measure,
): StackedBarChartBar[] {
  const unit = detail.units.find((candidate) => candidate.id === unitId);
  if (unit === undefined) return [];
  return unit.runs.map((run, index) => {
    const value = runValue(run, measure);
    const segment: AgentSegment = {
      key: keyFor(shortId(run.runId), value.unpriced),
      value: value.value,
      color: colorFor(index),
      runId: run.runId,
    };
    return {
      label: `${run.model ?? 'unknown model'} · ${shortId(run.runId)}`,
      segments: [segment],
    };
  });
}

function overviewSummary(plans: PlansResponse): string {
  const runIds = plans.plans.reduce((sum, plan) => sum + plan.runIds, 0);
  const resolved = plans.plans.reduce((sum, plan) => sum + plan.resolved, 0);
  return `${plans.plans.length} plans · ${resolved} of ${runIds} runIds joined to a run record`;
}

function detailSummary(detail: PlanDetail): string {
  return `${detail.resolved} of ${detail.runIds} runIds joined to a run record · ${detail.unpricedRuns} unpriced runs`;
}

export function AgentsDashboardScreen() {
  const [measure, setMeasure] = useState<Measure>('tokens');
  const [plansState, setPlansState] = useState<PlansState>({
    status: 'loading',
  });
  const [detailState, setDetailState] = useState<DetailState>({
    status: 'loading',
  });
  const [drill, setDrill] = useState<DrillState>({ level: 'overview' });

  const drillSlug = drill.level === 'overview' ? null : drill.slug;

  const { reportingGateway: overrideGateway } =
    useDependencyOverrides<{ reportingGateway: ReportingGateway }>(
      'dashboards',
    );
  const reportingGateway = useMemo(
    () =>
      overrideGateway ?? createHttpReportingGateway(appConfig.reportingBaseUrl),
    [overrideGateway],
  );

  useEffect(() => {
    const gateway = reportingGateway;
    let cancelled = false;
    void gateway.getPlans().then((result) => {
      if (cancelled) return;
      setPlansState(result);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (drillSlug === null) return;
    const gateway = reportingGateway;
    let cancelled = false;
    void gateway.getPlanDetail(drillSlug).then((result) => {
      if (cancelled) return;
      setDetailState(result);
    });
    return () => {
      cancelled = true;
    };
  }, [drillSlug]);

  const overviewBars = useMemo(
    () =>
      plansState.status === 'ready'
        ? buildOverviewBars(plansState.data, measure)
        : [],
    [plansState, measure],
  );

  const crumbs: Crumb[] =
    drill.level === 'overview'
      ? [{ label: 'All plans' }]
      : drill.level === 'plan'
        ? [
            {
              label: 'All plans',
              onPress: () => setDrill({ level: 'overview' }),
            },
            { label: `${drill.day} · ${drill.slug}` },
          ]
        : [
            {
              label: 'All plans',
              onPress: () => setDrill({ level: 'overview' }),
            },
            {
              label: `${drill.day} · ${drill.slug}`,
              onPress: () =>
                setDrill({ level: 'plan', day: drill.day, slug: drill.slug }),
            },
            {
              label:
                detailState.status === 'ready'
                  ? (detailState.data.units.find(
                      (unit) => unit.id === drill.unitId,
                    )?.title ?? drill.unitId)
                  : drill.unitId,
            },
          ];

  const renderOverview = () => {
    if (plansState.status === 'loading') {
      return (
        <View items="center" py="$6" testID="AgentsDashboardScreen-Loading">
          <Spinner testID="AgentsDashboardScreen-LoadingSpinner" />
        </View>
      );
    }
    if (plansState.status === 'not-configured') {
      return (
        <Text
          variant="muted"
          mx="$4"
          testID="AgentsDashboardScreen-NotConfigured"
        >
          Reporting service is not configured, so there is nothing to chart.
        </Text>
      );
    }
    if (plansState.status === 'unreachable') {
      return (
        <Text
          variant="muted"
          mx="$4"
          testID="AgentsDashboardScreen-Unreachable"
        >
          Reporting service unreachable: {plansState.reason}. Charts are
          unavailable until it recovers.
        </Text>
      );
    }
    return (
      <YStack gap="$3" px="$4" testID="AgentsDashboardScreen-Content">
        <Text variant="muted" testID="AgentsDashboardScreen-OverviewSummary">
          {overviewSummary(plansState.data)}
        </Text>
        <Card testID="AgentsOverviewCard">
          <CardHeader>
            <Text variant="large" testID="AgentsOverviewTitle">
              {MEASURE_LABEL[measure]} per day
            </Text>
          </CardHeader>
          <CardContent>
            <StackedBarChart
              data={overviewBars}
              onSegmentPress={(bar, segment) => {
                const agentSegment = segment as AgentSegment;
                if (agentSegment.slug === undefined) return;
                Logger.withMetadata({
                  day: bar.label,
                  slug: agentSegment.slug,
                }).info('dashboards.agents.drill_into_plan');
                setDetailState({ status: 'loading' });
                setDrill({
                  level: 'plan',
                  day: bar.label,
                  slug: agentSegment.slug,
                });
              }}
              height={220}
              emptyLabel="No plan activity recorded yet."
              testID="AgentsOverviewChart"
            />
          </CardContent>
        </Card>
      </YStack>
    );
  };

  const renderDetail = (
    current: Extract<DrillState, { level: 'plan' | 'unit' }>,
  ) => {
    if (detailState.status === 'loading') {
      return (
        <View
          items="center"
          py="$6"
          testID="AgentsDashboardScreen-DetailLoading"
        >
          <Spinner testID="AgentsDashboardScreen-DetailLoadingSpinner" />
        </View>
      );
    }
    if (detailState.status === 'not-configured') {
      return (
        <Text
          variant="muted"
          mx="$4"
          testID="AgentsDashboardScreen-DetailNotConfigured"
        >
          Reporting service is not configured, so there is nothing to chart.
        </Text>
      );
    }
    if (detailState.status === 'unreachable') {
      return (
        <Text
          variant="muted"
          mx="$4"
          testID="AgentsDashboardScreen-DetailUnreachable"
        >
          Reporting service unreachable: {detailState.reason}. Charts are
          unavailable until it recovers.
        </Text>
      );
    }
    return (
      <YStack gap="$3" px="$4" testID="AgentsDashboardScreen-DetailContent">
        <Text variant="muted" testID="AgentsDashboardScreen-DetailSummary">
          {detailSummary(detailState.data)}
        </Text>
        <Card testID="AgentsDetailCard">
          <CardHeader>
            <Text variant="large" testID="AgentsDetailTitle">
              {MEASURE_LABEL[measure]} ·{' '}
              {current.level === 'plan' ? 'units' : 'runs'}
            </Text>
          </CardHeader>
          <CardContent>
            {current.level === 'plan' ? (
              <StackedBarChart
                data={buildPlanBars(detailState.data, measure)}
                onSegmentPress={(_bar, segment) => {
                  const agentSegment = segment as AgentSegment;
                  if (agentSegment.unitId === undefined) return;
                  Logger.withMetadata({ unitId: agentSegment.unitId }).info(
                    'dashboards.agents.drill_into_unit',
                  );
                  setDrill({
                    level: 'unit',
                    day: current.day,
                    slug: current.slug,
                    unitId: agentSegment.unitId,
                  });
                }}
                height={220}
                emptyLabel="This plan has no joined runs yet."
                testID="AgentsPlanChart"
              />
            ) : (
              <StackedBarChart
                data={buildUnitBars(
                  detailState.data,
                  current.unitId,
                  measure,
                )}
                onSegmentPress={(_bar, segment) => {
                  const agentSegment = segment as AgentSegment;
                  if (agentSegment.runId === undefined) return;
                  Logger.withMetadata({ runId: agentSegment.runId }).info(
                    'dashboards.agents.open_run',
                  );
                  router.push(`/coding-agent/runs/${agentSegment.runId}`);
                }}
                height={220}
                emptyLabel="This unit has no joined runs yet."
                testID="AgentsUnitChart"
              />
            )}
          </CardContent>
        </Card>
      </YStack>
    );
  };

  return (
    <View flex={1} bg="$background" testID="AgentsDashboardScreen">
      <ScreenHeader
        testID="AgentsDashboardScreenHeader"
        title="Agents"
        onBack={() => {
          Logger.info('dashboards.agents.back', { to: '/dashboards' });
          backTo('/dashboards');
        }}
      />
      <ScrollView
        testID="AgentsDashboardScreen-ScrollView"
        contentContainerStyle={{ paddingVertical: 16, gap: 12 }}
      >
        <View mx="$4" testID="AgentsMeasureSelector">
          <SegmentedControl
            options={MEASURE_OPTIONS}
            value={measure}
            onChange={(value) => {
              Logger.info('dashboards.agents.measure_change', {
                measure: value,
              });
              setMeasure(value as Measure);
            }}
            pressLog="off"
            testID="AgentsMeasureSelectorControl"
          />
        </View>
        <XStack
          flexWrap="wrap"
          items="center"
          gap="$2"
          px="$4"
          testID="AgentsBreadcrumb"
        >
          {crumbs.map((crumb, index) => (
            <Fragment key={`${crumb.label}-${index}`}>
              {index > 0 ? (
                <Text variant="muted" testID="AgentsBreadcrumbSeparator">
                  ›
                </Text>
              ) : null}
              {crumb.onPress ? (
                <Text
                  variant="muted"
                  onPress={crumb.onPress}
                  pressStyle={{ opacity: 0.7 }}
                  cursor="pointer"
                  testID={`AgentsBreadcrumb-${index}`}
                >
                  {crumb.label}
                </Text>
              ) : (
                <Text variant="muted" testID={`AgentsBreadcrumb-${index}`}>
                  {crumb.label}
                </Text>
              )}
            </Fragment>
          ))}
        </XStack>
        {drill.level === 'overview' ? renderOverview() : renderDetail(drill)}
      </ScrollView>
    </View>
  );
}