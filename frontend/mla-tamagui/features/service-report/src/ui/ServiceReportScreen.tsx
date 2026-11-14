/**
 * Real `ServiceReportScreen` — fetches the health-report history from the
 * ORDS endpoint (`useHealthReportHistory`) and renders the dashboard: a
 * latest-report summary, error/uptime/log-level charts, service status
 * grid, recent logs/traces, and test-tier pass/fail counts.
 *
 * The "not configured" and "unreachable" states are rendered distinctly on
 * purpose: the Autonomous DB hasn't been provisioned yet, so a fresh
 * worktree is expected to show "not configured" indefinitely until that
 * lands — that is not the same operational situation as a real fetch
 * failure against a live endpoint, and the copy says so.
 */

import { Logger } from '@lib/logging';
import {
  errorTrendSeries,
  goldenScreenshotHistory,
  logLevelCounts,
  serviceUptimeSummary,
  summarizeServiceCounts,
  type HealthReport,
  type LogLine,
  type PackageTestResults,
  type RecentErrorSummary,
  type ServiceUptimeSummary,
  type SpanLine,
  type TestTierResult,
} from '../domain';
import {
  Admonition,
  Badge,
  type BadgeProps,
  BarChart,
  Button,
  Card,
  CardContent,
  CardHeader,
  LineChart,
  ListItem,
  PieChart,
  StatTile,
  Text,
  View,
} from '@lib/tamagui-design-system';
import { Clock, Server, TriangleAlert } from '@lib/tamagui-design-system/icons';
import React from 'react';
import { ActivityIndicator, Pressable, ScrollView } from 'react-native';

import { useHealthReportHistory } from '../state/useHealthReportHistory';

import { DrillDialog, type DrillItem, type DrillState } from './DrillDialog';
import {
  GoldenScreenshotViewer,
  type GoldenScreenshotViewerState,
} from './GoldenScreenshotViewer';

/**
 * Badge has no dedicated `warning` variant (see design-system/ui/badge.tsx) —
 * `secondary` is the closest existing token for a caution level that isn't
 * an outright error.
 */
function levelBadgeVariant(level: string): BadgeProps['variant'] {
  const normalized = level.toLowerCase();
  if (normalized === 'error') return 'destructive';
  if (normalized === 'warn' || normalized === 'warning') return 'secondary';
  return 'outline';
}

function uptimeBarColor(
  percent: number
): 'success' | 'warning' | 'destructive' {
  if (percent === 100) return 'success';
  if (percent > 0) return 'warning';
  return 'destructive';
}

function logLevelColor(level: string): 'destructive' | 'warning' | 'muted' {
  if (level === 'error') return 'destructive';
  if (level === 'warn' || level === 'warning') return 'warning';
  return 'muted';
}

/**
 * Short, recognizable chart-axis labels for each VM app — the full name is
 * already shown in the Services card right below, so the bar chart only
 * needs enough to disambiguate at a glance without gifted-charts truncating
 * mid-word (confirmed: a naive slice() cut a long app name to its first
 * ten characters, which read as garbled overlapping text on a narrow chart).
 */
const SERVICE_SHORT_LABELS: Record<string, string> = {
  'leetcode-backend': 'backend',
  'reporting-service': 'reports',
  'remote-chat-agent': 'agent',
  'leetcode-auth-cookie-capture': 'capture',
  'vm-gateway': 'gateway',
};

function serviceShortLabel(app: string): string {
  return SERVICE_SHORT_LABELS[app] ?? app.slice(0, 8);
}

// ---------------------------------------------------------------------------
// Drill-state builders — each converts a domain slice into the generic
// DrillDialog shape. Kept as pure functions so the drill content for a given
// tap is fully determined by its inputs (easy to reason about, easy to test).
// ---------------------------------------------------------------------------

function buildErrorSnapshotDrill(report: HealthReport): DrillState {
  return {
    title: `Errors — ${report.generatedAt}`,
    items: report.recentErrors.map((e: RecentErrorSummary) => ({
      id: `${report.generatedAt}-${e.source}`,
      title: e.source,
      subtitle: `${e.errorCount} error-level row${e.errorCount === 1 ? '' : 's'} (last ${e.windowHours}h)`,
      badge: {
        label: String(e.errorCount),
        variant: e.errorCount > 0 ? 'destructive' : 'success',
      },
      raw: {
        source: e.source,
        windowHours: e.windowHours,
        errorCount: e.errorCount,
        sampleMessages: e.sampleMessages.join(' | ') || '(none)',
        error: e.error ?? '(none)',
      },
    })),
  };
}

function buildUptimeDrill(app: string, reports: HealthReport[]): DrillState {
  const rows = reports
    .map((report) => ({
      report,
      entry: report.services.find((s) => s.app === app),
    }))
    .filter(
      (r): r is { report: HealthReport; entry: typeof r.entry & {} } =>
        r.entry !== undefined
    );

  return {
    title: `${app} — uptime history`,
    items: rows.map(({ report, entry }) => ({
      id: `${app}-${report.generatedAt}`,
      title: report.generatedAt,
      subtitle: entry.reachable ? 'up' : (entry.error ?? 'down'),
      badge: {
        label: entry.reachable ? 'up' : 'down',
        variant: entry.reachable ? 'success' : 'destructive',
      },
      raw: {
        generatedAt: report.generatedAt,
        reachable: entry.reachable,
        error: entry.error ?? '(none)',
        containers:
          entry.containers
            ?.map((c) => `${c.name}: ${c.state} (${c.status})`)
            .join(' | ') || '(none reported)',
      },
    })),
  };
}

function buildLogLevelDrill(level: string, lines: LogLine[]): DrillState {
  const filtered = lines.filter((l) => l.level.toLowerCase() === level);
  return {
    title: `${level} logs`,
    items: filtered.map((l, index) => ({
      id: `${level}-${l.dt}-${index}`,
      title: l.module,
      subtitle: l.dt,
      badge: { label: l.level, variant: levelBadgeVariant(l.level) },
      raw: { dt: l.dt, level: l.level, module: l.module, message: l.message },
    })),
  };
}

function logLineToDrillItem(line: LogLine, index: number): DrillItem {
  return {
    id: `log-${line.dt}-${index}`,
    title: line.module,
    subtitle: line.dt,
    badge: { label: line.level, variant: levelBadgeVariant(line.level) },
    raw: {
      dt: line.dt,
      level: line.level,
      module: line.module,
      message: line.message,
    },
  };
}

function spanToDrillItem(span: SpanLine, index: number): DrillItem {
  return {
    id: `span-${span.dt}-${index}`,
    title: span.name,
    subtitle: `${span.service} — ${span.durationMs}ms`,
    badge:
      span.statusCode !== undefined
        ? {
            label: String(span.statusCode),
            variant:
              span.statusCode >= 500
                ? 'destructive'
                : span.statusCode >= 400
                  ? 'secondary'
                  : 'success',
          }
        : undefined,
    raw: {
      dt: span.dt,
      service: span.service,
      name: span.name,
      durationMs: span.durationMs,
      statusCode: span.statusCode ?? '(none)',
    },
  };
}

function caseStatusBadgeVariant(
  status: 'passed' | 'failed' | 'skipped'
): BadgeProps['variant'] {
  if (status === 'passed') return 'success';
  if (status === 'failed') return 'destructive';
  return 'outline';
}

/** Level-1 list for one package's one non-unit test tier: every case, or a single "unavailable" row. */
function buildTestTierDrill(pkgName: string, tier: TestTierResult): DrillState {
  if (tier.unavailableReason) {
    return {
      title: `${pkgName} — ${tier.tier}`,
      items: [
        {
          id: `${pkgName}-${tier.tier}-unavailable`,
          title: 'Unavailable in this environment',
          subtitle: tier.unavailableReason,
          badge: { label: 'skipped', variant: 'outline' },
          raw: { reason: tier.unavailableReason },
        },
      ],
    };
  }
  const cases = tier.cases ?? [];
  return {
    title: `${pkgName} — ${tier.tier} (${tier.testsPassed ?? '?'}/${tier.testsTotal ?? '?'})`,
    items: cases.map((c, index) => ({
      id: `${pkgName}-${tier.tier}-${index}`,
      title: c.name,
      subtitle: c.suite,
      badge: { label: c.status, variant: caseStatusBadgeVariant(c.status) },
      raw: {
        suite: c.suite,
        name: c.name,
        status: c.status,
        durationMs: c.durationMs ?? '(n/a)',
        failureMessage: c.failureMessage ?? '(none)',
      },
    })),
  };
}

// ---------------------------------------------------------------------------
// Charts
// ---------------------------------------------------------------------------

function ErrorTrendChart({
  reports,
  onPointPress,
}: {
  reports: HealthReport[];
  onPointPress: (report: HealthReport) => void;
}) {
  const points = errorTrendSeries(reports);

  if (points.length === 0) {
    return (
      <Text variant="muted" testID="ServiceReportScreen-ErrorTrendChart-Empty">
        No history yet.
      </Text>
    );
  }

  const hasErrors = points.some((p) => p.totalErrors > 0);
  const caption = `Total error count across last ${points.length} snapshot${points.length === 1 ? '' : 's'} — tap a point for details`;

  if (points.length === 1) {
    const report = reports[0];
    return (
      <View
        bg="$color3"
        gap="$2"
        py="$2"
        testID="ServiceReportScreen-ErrorTrendChart"
      >
        <Pressable
          onPress={
            report
              ? () => {
                  onPointPress(report);
                }
              : undefined
          }
          testID="ServiceReportScreen-ErrorTrendChart-SinglePoint-Pressable"
        >
          <Badge
            variant={hasErrors ? 'destructive' : 'success'}
            testID="ServiceReportScreen-ErrorTrendChart-SinglePoint"
          >
            {`${points[0].totalErrors} errors`}
          </Badge>
        </Pressable>
        <Text
          variant="muted"
          testID="ServiceReportScreen-ErrorTrendChart-Caption"
        >
          Total error count across last snapshot — trend needs at least 2 to
          draw a line
        </Text>
      </View>
    );
  }

  // errorTrendSeries returns oldest-first (chart order); reports is newest-first.
  const oldestFirstReports = [...reports].reverse();

  return (
    <View bg="$color3" testID="ServiceReportScreen-ErrorTrendChart">
      <LineChart
        data={points.map((p, index) => ({
          value: p.totalErrors,
          onPress: () => {
            const report = oldestFirstReports[index];
            if (report) onPointPress(report);
          },
        }))}
        color={hasErrors ? 'destructive' : 'success'}
        testID="ServiceReportScreen-ErrorTrendChart-LineChart"
      />
      <Text
        variant="muted"
        testID="ServiceReportScreen-ErrorTrendChart-Caption"
      >
        {caption}
      </Text>
    </View>
  );
}

function ServiceUptimeChart({
  summary,
  onBarPress,
}: {
  summary: ServiceUptimeSummary[];
  onBarPress: (app: string) => void;
}) {
  if (summary.length === 0) {
    return (
      <Text variant="muted" testID="ServiceReportScreen-UptimeChart-Empty">
        No service history yet.
      </Text>
    );
  }
  return (
    <View bg="$color3" testID="ServiceReportScreen-UptimeChart">
      <BarChart
        data={summary.map((s) => ({
          label: serviceShortLabel(s.app),
          value: s.upPercent,
          color: uptimeBarColor(s.upPercent),
          onPress: () => {
            onBarPress(s.app);
          },
        }))}
        testID="ServiceReportScreen-UptimeChart-BarChart"
      />
      <Text variant="muted" testID="ServiceReportScreen-UptimeChart-Caption">
        {`Uptime % across last ${summary[0]?.totalCount ?? 0} snapshot(s) per service — tap a bar for details`}
      </Text>
    </View>
  );
}

function LogLevelsChart({
  lines,
  onSlicePress,
}: {
  lines: LogLine[];
  onSlicePress: (level: string) => void;
}) {
  const counts = logLevelCounts(lines);
  if (counts.length === 0) {
    return (
      <Text variant="muted" testID="ServiceReportScreen-LogLevelsChart-Empty">
        No recent log activity.
      </Text>
    );
  }
  return (
    <View bg="$color3" testID="ServiceReportScreen-LogLevelsChart">
      <PieChart
        data={counts.map((c) => ({
          label: c.level,
          value: c.count,
          color: logLevelColor(c.level),
          onPress: () => {
            onSlicePress(c.level);
          },
        }))}
        testID="ServiceReportScreen-LogLevelsChart-PieChart"
      />
      <Text variant="muted" testID="ServiceReportScreen-LogLevelsChart-Caption">
        Recent log lines by level — tap a slice or legend row for details
      </Text>
    </View>
  );
}

// ---------------------------------------------------------------------------
// Lists
// ---------------------------------------------------------------------------

function ServiceStatusGrid({
  report,
  onServicePress,
}: {
  report: HealthReport;
  onServicePress: (app: string) => void;
}) {
  return (
    <View gap="$2" testID="ServiceReportScreen-ServiceStatusGrid">
      {report.services.map((service) => (
        <ListItem
          key={service.app}
          onPress={() => {
            onServicePress(service.app);
          }}
          testID={`ServiceReportScreen-ServiceRow-${service.app}`}
        >
          <View testID={`ServiceReportScreen-ServiceRow-${service.app}-Row`}>
            <View
              gap="$0.5"
              testID={`ServiceReportScreen-ServiceRow-${service.app}-Info`}
            >
              <Text
                variant="strong"
                testID={`ServiceReportScreen-ServiceRow-${service.app}-Name`}
              >
                {service.app}
              </Text>
              {service.containers?.map((container) => (
                <Text
                  key={container.name}
                  variant="muted"
                  testID={`ServiceReportScreen-ServiceRow-${service.app}-Container-${container.name}`}
                >
                  {`${container.name} — ${container.state} (${container.status})`}
                </Text>
              ))}
              {service.error ? (
                <Text
                  variant="muted"
                  testID={`ServiceReportScreen-ServiceRow-${service.app}-Error`}
                >
                  {service.error}
                </Text>
              ) : null}
            </View>
            <Badge
              variant={service.reachable ? 'success' : 'destructive'}
              testID={`ServiceReportScreen-ServiceRow-${service.app}-Badge`}
            >
              {service.reachable ? 'up' : 'down'}
            </Badge>
          </View>
        </ListItem>
      ))}
    </View>
  );
}

function RecentLogsList({
  lines,
  onLinePress,
}: {
  lines: LogLine[];
  onLinePress: (item: DrillItem) => void;
}) {
  if (lines.length === 0) {
    return (
      <Text variant="muted" testID="ServiceReportScreen-RecentLogs-Empty">
        No recent log activity.
      </Text>
    );
  }

  return (
    <View gap="$2" testID="ServiceReportScreen-RecentLogs-List">
      {lines.map((line, index) => (
        <ListItem
          // eslint-disable-next-line react/no-array-index-key -- dt/module/message can repeat; index disambiguates
          key={`${line.dt}-${index}`}
          onPress={() => {
            onLinePress(logLineToDrillItem(line, index));
          }}
          testID={`ServiceReportScreen-RecentLogs-Row-${index}`}
        >
          <View
            gap="$1"
            testID={`ServiceReportScreen-RecentLogs-Row-${index}-Content`}
          >
            <View
              gap="$2"
              testID={`ServiceReportScreen-RecentLogs-Row-${index}-Header`}
            >
              <View
                gap="$2"
                testID={`ServiceReportScreen-RecentLogs-Row-${index}-Meta`}
              >
                <Badge
                  variant={levelBadgeVariant(line.level)}
                  testID={`ServiceReportScreen-RecentLogs-Row-${index}-LevelBadge`}
                >
                  {line.level}
                </Badge>
                <Text
                  variant="strong"
                  testID={`ServiceReportScreen-RecentLogs-Row-${index}-Module`}
                >
                  {line.module}
                </Text>
              </View>
              <Text
                variant="muted"
                testID={`ServiceReportScreen-RecentLogs-Row-${index}-Timestamp`}
              >
                {line.dt}
              </Text>
            </View>
            <Text
              testID={`ServiceReportScreen-RecentLogs-Row-${index}-Message`}
            >
              {line.message}
            </Text>
          </View>
        </ListItem>
      ))}
    </View>
  );
}

function RecentTracesList({
  spans,
  onSpanPress,
}: {
  spans: SpanLine[];
  onSpanPress: (item: DrillItem) => void;
}) {
  if (spans.length === 0) {
    return (
      <Text variant="muted" testID="ServiceReportScreen-RecentTraces-Empty">
        No recent trace activity.
      </Text>
    );
  }

  return (
    <View gap="$2" testID="ServiceReportScreen-RecentTraces-List">
      {spans.map((span, index) => (
        <ListItem
          // eslint-disable-next-line react/no-array-index-key -- dt/service/name can repeat; index disambiguates
          key={`${span.dt}-${index}`}
          onPress={() => {
            onSpanPress(spanToDrillItem(span, index));
          }}
          testID={`ServiceReportScreen-RecentTraces-Row-${index}`}
        >
          <View
            gap="$2"
            testID={`ServiceReportScreen-RecentTraces-Row-${index}-Row`}
          >
            <View
              gap="$0.5"
              testID={`ServiceReportScreen-RecentTraces-Row-${index}-Info`}
            >
              <Text
                variant="strong"
                testID={`ServiceReportScreen-RecentTraces-Row-${index}-Name`}
              >
                {span.name}
              </Text>
              <Text
                variant="muted"
                testID={`ServiceReportScreen-RecentTraces-Row-${index}-Meta`}
              >
                {`${span.service} · ${span.dt}`}
              </Text>
            </View>
            <Badge
              variant={
                span.statusCode !== undefined && span.statusCode >= 400
                  ? 'destructive'
                  : 'success'
              }
              testID={`ServiceReportScreen-RecentTraces-Row-${index}-Duration`}
            >
              {`${span.durationMs}ms`}
            </Badge>
          </View>
        </ListItem>
      ))}
    </View>
  );
}

function TestTierSummary({ report }: { report: HealthReport }) {
  if (!report.tests || report.tests.length === 0) return null;
  return (
    <View gap="$2" testID="ServiceReportScreen-TestTierSummary">
      {report.tests.map((pkg) => (
        <View
          key={pkg.pkgName}
          gap="$1"
          testID={`ServiceReportScreen-TestTierSummary-${pkg.pkgName}`}
        >
          <Text
            variant="strong"
            testID={`ServiceReportScreen-TestTierSummary-${pkg.pkgName}-Name`}
          >
            {pkg.pkgName}
          </Text>
          {pkg.tiers.map((tier) => (
            <View
              key={tier.tier}
              testID={`ServiceReportScreen-TestTierSummary-${pkg.pkgName}-${tier.tier}`}
            >
              <Text
                variant="muted"
                testID={`ServiceReportScreen-TestTierSummary-${pkg.pkgName}-${tier.tier}-Label`}
              >
                {tier.tier}
              </Text>
              <Badge
                variant={tier.ranOk ? 'success' : 'destructive'}
                testID={`ServiceReportScreen-TestTierSummary-${pkg.pkgName}-${tier.tier}-Badge`}
              >
                {`${tier.testsPassed ?? '?'}/${tier.testsTotal ?? '?'} tests`}
              </Badge>
            </View>
          ))}
        </View>
      ))}
    </View>
  );
}

/**
 * Runbook/e2e/integration/flow coverage, broken down by test suite and test
 * case (deliberately excludes the `unit` tier -- that stays in the plain
 * pass/fail counts above). Tapping a tier row opens the shared DrillDialog
 * at Level 1 (every case in that tier); a case row drills to Level 2
 * (formatted detail) and "View raw JSON" to Level 3, exactly like every
 * other entry point on this screen.
 */
function TestCoverageList({
  tests,
  onTierPress,
}: {
  tests: PackageTestResults[];
  onTierPress: (pkgName: string, tier: TestTierResult) => void;
}) {
  const rows = tests.flatMap((pkg) =>
    pkg.tiers
      // `golden` gets its own dedicated card (GoldenScreenshotsList) with an
      // image-aware viewer, rather than this generic raw-field drill.
      .filter((tier) => tier.tier !== 'unit' && tier.tier !== 'golden')
      .map((tier) => ({ pkgName: pkg.pkgName, tier }))
  );

  if (rows.length === 0) {
    return (
      <Text variant="muted" testID="ServiceReportScreen-TestCoverage-Empty">
        No e2e, integration, flow or runbook-proxy tiers recorded.
      </Text>
    );
  }

  return (
    <View gap="$2" testID="ServiceReportScreen-TestCoverage-List">
      {rows.map(({ pkgName, tier }) => (
        <ListItem
          key={`${pkgName}-${tier.tier}`}
          onPress={() => {
            onTierPress(pkgName, tier);
          }}
          testID={`ServiceReportScreen-TestCoverage-Row-${pkgName}-${tier.tier}`}
        >
          <View
            testID={`ServiceReportScreen-TestCoverage-Row-${pkgName}-${tier.tier}-Row`}
          >
            <View
              gap="$0.5"
              testID={`ServiceReportScreen-TestCoverage-Row-${pkgName}-${tier.tier}-Info`}
            >
              <Text
                variant="strong"
                testID={`ServiceReportScreen-TestCoverage-Row-${pkgName}-${tier.tier}-Name`}
              >
                {pkgName}
              </Text>
              <Text
                variant="muted"
                testID={`ServiceReportScreen-TestCoverage-Row-${pkgName}-${tier.tier}-Tier`}
              >
                {tier.tier}
              </Text>
            </View>
            <Badge
              variant={
                tier.unavailableReason
                  ? 'outline'
                  : tier.ranOk
                    ? 'success'
                    : 'destructive'
              }
              testID={`ServiceReportScreen-TestCoverage-Row-${pkgName}-${tier.tier}-Badge`}
            >
              {tier.unavailableReason
                ? 'skipped'
                : `${tier.testsPassed ?? '?'}/${tier.testsTotal ?? '?'}`}
            </Badge>
          </View>
        </ListItem>
      ))}
    </View>
  );
}

/**
 * Every `golden`-tier case across all packages in the latest report, tapping
 * a row opens `GoldenScreenshotViewer` — a history + compare + comments
 * surface — rather than the generic raw-field drill every other tier uses.
 */
function GoldenScreenshotsList({
  tests,
  onCasePress,
}: {
  tests: PackageTestResults[];
  onCasePress: (pkgName: string, suite: string, name: string) => void;
}) {
  const rows = tests.flatMap((pkg) => {
    const tier = pkg.tiers.find((t) => t.tier === 'golden');
    return (tier?.cases ?? []).map((testCase) => ({
      pkgName: pkg.pkgName,
      testCase,
    }));
  });

  if (rows.length === 0) {
    return (
      <Text
        variant="muted"
        testID="ServiceReportScreen-GoldenScreenshots-Empty"
      >
        No golden screenshot tier recorded.
      </Text>
    );
  }

  return (
    <View gap="$2" testID="ServiceReportScreen-GoldenScreenshots-List">
      {rows.map(({ pkgName, testCase }) => (
        <ListItem
          key={`${pkgName}-${testCase.suite}-${testCase.name}`}
          onPress={() => {
            onCasePress(pkgName, testCase.suite, testCase.name);
          }}
          testID={`ServiceReportScreen-GoldenScreenshots-Row-${pkgName}-${testCase.name}`}
        >
          <View
            testID={`ServiceReportScreen-GoldenScreenshots-Row-${pkgName}-${testCase.name}-Row`}
          >
            <View
              gap="$0.5"
              testID={`ServiceReportScreen-GoldenScreenshots-Row-${pkgName}-${testCase.name}-Info`}
            >
              <Text
                variant="strong"
                testID={`ServiceReportScreen-GoldenScreenshots-Row-${pkgName}-${testCase.name}-Name`}
              >
                {testCase.name}
              </Text>
              <Text
                variant="muted"
                testID={`ServiceReportScreen-GoldenScreenshots-Row-${pkgName}-${testCase.name}-Suite`}
              >
                {testCase.suite}
              </Text>
            </View>
            <Badge
              variant={caseStatusBadgeVariant(testCase.status)}
              testID={`ServiceReportScreen-GoldenScreenshots-Row-${pkgName}-${testCase.name}-Badge`}
            >
              {testCase.status}
            </Badge>
          </View>
        </ListItem>
      ))}
    </View>
  );
}

function RefreshButton({
  onPress,
  testID,
}: {
  onPress: () => void;
  testID: string;
}) {
  return (
    <Button
      variant="secondary"
      testID={testID}
      onPress={() => {
        Logger.info('service_report.refresh_pressed');
        onPress();
      }}
    >
      Refresh
    </Button>
  );
}

function HealthReportDashboard({
  reports,
  onRefresh,
}: {
  reports: HealthReport[];
  onRefresh: () => void;
}) {
  const [drill, setDrill] = React.useState<DrillState>(null);
  const [goldenViewer, setGoldenViewer] =
    React.useState<GoldenScreenshotViewerState>(null);

  const goldenHistory = React.useMemo(
    () =>
      goldenViewer
        ? goldenScreenshotHistory(
            reports,
            goldenViewer.pkgName,
            goldenViewer.suite,
            goldenViewer.name
          )
        : [],
    [goldenViewer, reports]
  );

  const selectItem = React.useCallback((item: DrillItem) => {
    setDrill((prev) =>
      prev ? { ...prev, selected: item, showRaw: false } : prev
    );
  }, []);
  const showRaw = React.useCallback(() => {
    setDrill((prev) => (prev ? { ...prev, showRaw: true } : prev));
  }, []);
  const goBack = React.useCallback(() => {
    setDrill((prev) => {
      if (!prev) return prev;
      if (prev.showRaw) return { ...prev, showRaw: false };
      if (prev.selected) return { ...prev, selected: undefined };
      return prev;
    });
  }, []);
  const closeDrill = React.useCallback(() => {
    setDrill(null);
  }, []);

  const latest = reports[0];
  if (!latest) {
    return (
      <View gap="$4" p="$6" testID="ServiceReportScreen">
        <Text variant="muted" testID="ServiceReportScreen-EmptyText">
          No health reports available yet.
        </Text>
        <RefreshButton
          onPress={onRefresh}
          testID="ServiceReportScreen-RefreshButton"
        />
      </View>
    );
  }

  const counts = summarizeServiceCounts(latest.services);
  const uptimeSummary = serviceUptimeSummary(reports);
  const totalRecentErrors = latest.recentErrors.reduce(
    (sum, e) => sum + e.errorCount,
    0
  );

  return (
    <ScrollView
      style={{ flex: 1 }}
      contentContainerStyle={{ gap: 16, padding: 16 }}
      testID="ServiceReportScreen"
    >
      <View testID="ServiceReportScreen-HeaderRow">
        <Text variant="h2" testID="ServiceReportScreen-Heading">
          Service health
        </Text>
        <RefreshButton
          onPress={onRefresh}
          testID="ServiceReportScreen-RefreshButton"
        />
      </View>

      <Card testID="ServiceReportScreen-SummaryCard">
        <CardHeader>
          <Text variant="h3" testID="ServiceReportScreen-SummaryCard-Title">
            Latest snapshot
          </Text>
          <Text
            variant="muted"
            testID="ServiceReportScreen-SummaryCard-GeneratedAt"
          >
            {`Generated ${latest.generatedAt}`}
          </Text>
        </CardHeader>
        <CardContent gap="$3">
          <View gap="$2" testID="ServiceReportScreen-SummaryCard-VmOpsRow">
            <Text testID="ServiceReportScreen-SummaryCard-VmOpsLabel">
              vm-ops-api:
            </Text>
            <Badge
              variant={latest.vmOps.reachable ? 'success' : 'destructive'}
              testID="ServiceReportScreen-SummaryCard-VmOpsBadge"
            >
              {latest.vmOps.reachable ? 'reachable' : 'unreachable'}
            </Badge>
          </View>
          <View gap="$3" testID="ServiceReportScreen-SummaryCard-StatRow">
            <StatTile
              icon={Server}
              value={`${counts.up}/${counts.total}`}
              label="services up"
            />
            <StatTile
              icon={TriangleAlert}
              value={String(totalRecentErrors)}
              label="errors (24h)"
            />
            <StatTile
              icon={Clock}
              value={String(reports.length)}
              label="snapshots"
            />
          </View>
        </CardContent>
      </Card>

      <Card testID="ServiceReportScreen-ErrorTrendCard">
        <CardHeader>
          <Text variant="h3" testID="ServiceReportScreen-ErrorTrendCard-Title">
            Error trend
          </Text>
        </CardHeader>
        <CardContent>
          <ErrorTrendChart
            reports={reports}
            onPointPress={(report) => {
              setDrill(buildErrorSnapshotDrill(report));
            }}
          />
        </CardContent>
      </Card>

      <Card testID="ServiceReportScreen-UptimeCard">
        <CardHeader>
          <Text variant="h3" testID="ServiceReportScreen-UptimeCard-Title">
            Service uptime
          </Text>
        </CardHeader>
        <CardContent>
          <ServiceUptimeChart
            summary={uptimeSummary}
            onBarPress={(app) => {
              setDrill(buildUptimeDrill(app, reports));
            }}
          />
        </CardContent>
      </Card>

      <Card testID="ServiceReportScreen-LogLevelsCard">
        <CardHeader>
          <Text variant="h3" testID="ServiceReportScreen-LogLevelsCard-Title">
            Log levels
          </Text>
        </CardHeader>
        <CardContent>
          <LogLevelsChart
            lines={latest.recentLogLines ?? []}
            onSlicePress={(level) => {
              setDrill(buildLogLevelDrill(level, latest.recentLogLines ?? []));
            }}
          />
        </CardContent>
      </Card>

      <Card testID="ServiceReportScreen-ServicesCard">
        <CardHeader>
          <Text variant="h3" testID="ServiceReportScreen-ServicesCard-Title">
            Services
          </Text>
        </CardHeader>
        <CardContent>
          <ServiceStatusGrid
            report={latest}
            onServicePress={(app) => {
              setDrill(buildUptimeDrill(app, reports));
            }}
          />
        </CardContent>
      </Card>

      <Card testID="ServiceReportScreen-RecentLogsCard">
        <CardHeader>
          <Text variant="h3" testID="ServiceReportScreen-RecentLogsCard-Title">
            Recent logs
          </Text>
        </CardHeader>
        <CardContent>
          <RecentLogsList
            lines={latest.recentLogLines ?? []}
            onLinePress={(item) => {
              setDrill({ title: item.title, items: [item], selected: item });
            }}
          />
        </CardContent>
      </Card>

      <Card testID="ServiceReportScreen-RecentTracesCard">
        <CardHeader>
          <Text
            variant="h3"
            testID="ServiceReportScreen-RecentTracesCard-Title"
          >
            Recent traces
          </Text>
        </CardHeader>
        <CardContent>
          <RecentTracesList
            spans={latest.recentSpans ?? []}
            onSpanPress={(item) => {
              setDrill({ title: item.title, items: [item], selected: item });
            }}
          />
        </CardContent>
      </Card>

      {latest.tests && latest.tests.length > 0 ? (
        <Card testID="ServiceReportScreen-TestsCard">
          <CardHeader>
            <Text variant="h3" testID="ServiceReportScreen-TestsCard-Title">
              Test tiers
            </Text>
          </CardHeader>
          <CardContent>
            <TestTierSummary report={latest} />
          </CardContent>
        </Card>
      ) : null}

      <Card testID="ServiceReportScreen-GoldenScreenshotsCard">
        <CardHeader>
          <Text
            variant="h3"
            testID="ServiceReportScreen-GoldenScreenshotsCard-Title"
          >
            Golden screenshots
          </Text>
          <Text
            variant="muted"
            testID="ServiceReportScreen-GoldenScreenshotsCard-Subtitle"
          >
            Tap a case to compare against its baseline, browse run history, and
            comment
          </Text>
        </CardHeader>
        <CardContent>
          <GoldenScreenshotsList
            tests={latest.tests ?? []}
            onCasePress={(pkgName, suite, name) => {
              setGoldenViewer({ pkgName, suite, name });
            }}
          />
        </CardContent>
      </Card>

      <Card testID="ServiceReportScreen-TestCoverageCard">
        <CardHeader>
          <Text
            variant="h3"
            testID="ServiceReportScreen-TestCoverageCard-Title"
          >
            Test coverage
          </Text>
          <Text
            variant="muted"
            testID="ServiceReportScreen-TestCoverageCard-Subtitle"
          >
            Runbook / e2e / integration / flow — tap a tier for suite and case
            detail
          </Text>
        </CardHeader>
        <CardContent>
          <TestCoverageList
            tests={latest.tests ?? []}
            onTierPress={(pkgName, tier) => {
              setDrill(buildTestTierDrill(pkgName, tier));
            }}
          />
        </CardContent>
      </Card>

      <DrillDialog
        state={drill}
        onClose={closeDrill}
        onSelect={selectItem}
        onShowRaw={showRaw}
        onBack={goBack}
      />

      <GoldenScreenshotViewer
        state={goldenViewer}
        history={goldenHistory}
        onClose={() => {
          setGoldenViewer(null);
        }}
      />
    </ScrollView>
  );
}

function ServiceReportScreen(): React.ReactElement {
  const history = useHealthReportHistory();

  if (history.status === 'loading') {
    return (
      <View gap="$3" p="$6" testID="ServiceReportScreen">
        <ActivityIndicator testID="ServiceReportScreen-LoadingIndicator" />
        <Text variant="muted" testID="ServiceReportScreen-LoadingText">
          Loading service health history…
        </Text>
      </View>
    );
  }

  if (history.status === 'not-configured') {
    return (
      <View gap="$4" p="$6" testID="ServiceReportScreen">
        <Admonition
          variant="info"
          testID="ServiceReportScreen-NotConfiguredAdmonition"
        >
          <Admonition.Title>Not configured yet</Admonition.Title>
          <Admonition.Body>{history.reason}</Admonition.Body>
        </Admonition>
        <RefreshButton
          onPress={history.refresh}
          testID="ServiceReportScreen-RefreshButton"
        />
      </View>
    );
  }

  if (history.status === 'unreachable') {
    return (
      <View gap="$4" p="$6" testID="ServiceReportScreen">
        <Admonition
          variant="error"
          testID="ServiceReportScreen-UnreachableAdmonition"
        >
          <Admonition.Title>
            Could not reach the health-reports endpoint
          </Admonition.Title>
          <Admonition.Body>{history.error}</Admonition.Body>
        </Admonition>
        <RefreshButton
          onPress={history.refresh}
          testID="ServiceReportScreen-RefreshButton"
        />
      </View>
    );
  }

  return (
    <HealthReportDashboard
      reports={history.reports}
      onRefresh={history.refresh}
    />
  );
}

export { ServiceReportScreen };
