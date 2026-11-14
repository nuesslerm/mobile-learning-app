/**
 * Pure calculations derived from `HealthReport` data, unit-testable without
 * React/RN.
 */

import type {
  HealthReport,
  LogLine,
  ServiceHealthEntry,
  TestCaseResult,
} from './healthReport';

/** Newest-first — the only ordering the test fixtures and the real ORDS client are expected to hand in. */
export function sortReportsByGeneratedAtDesc(
  reports: HealthReport[]
): HealthReport[] {
  return [...reports].sort(
    (a, b) => Date.parse(b.generatedAt) - Date.parse(a.generatedAt)
  );
}

export type ServiceCounts = { up: number; down: number; total: number };

export function summarizeServiceCounts(
  services: ServiceHealthEntry[]
): ServiceCounts {
  const up = services.filter((s) => s.reachable).length;
  return { up, down: services.length - up, total: services.length };
}

export type ErrorTrendPoint = { generatedAt: string; totalErrors: number };

/** Total `recentErrors[].errorCount` per report, oldest-first (left-to-right for a chart). */
export function errorTrendSeries(
  reportsNewestFirst: HealthReport[]
): ErrorTrendPoint[] {
  return [...reportsNewestFirst].reverse().map((report) => ({
    generatedAt: report.generatedAt,
    totalErrors: report.recentErrors.reduce(
      (sum, entry) => sum + entry.errorCount,
      0
    ),
  }));
}

export type ServiceUptimeSummary = {
  app: string;
  upCount: number;
  totalCount: number;
  upPercent: number;
};

/**
 * Per-app uptime across every given report snapshot: what fraction of
 * snapshots saw that app `reachable`. Order follows first-seen app name
 * across the (newest-first) report list, so a service present in the
 * newest snapshot always sorts first.
 */
export function serviceUptimeSummary(
  reports: HealthReport[]
): ServiceUptimeSummary[] {
  const order: string[] = [];
  const counts = new Map<string, { up: number; total: number }>();
  for (const report of reports) {
    for (const service of report.services) {
      if (!counts.has(service.app)) {
        counts.set(service.app, { up: 0, total: 0 });
        order.push(service.app);
      }
      const entry = counts.get(service.app);
      if (!entry) continue;
      entry.total += 1;
      if (service.reachable) entry.up += 1;
    }
  }
  return order.map((app) => {
    const entry = counts.get(app) ?? { up: 0, total: 0 };
    return {
      app,
      upCount: entry.up,
      totalCount: entry.total,
      upPercent:
        entry.total > 0 ? Math.round((entry.up / entry.total) * 100) : 0,
    };
  });
}

export type GoldenScreenshotHistoryEntry = {
  generatedAt: string;
  testCase: TestCaseResult;
};

/**
 * Every historical capture of one golden test case (matched by package +
 * suite + name) across `reportsNewestFirst`, newest first — the "screen
 * history" a golden-test row's tap opens (swipe/side-by-side/overlay compare
 * + comments). A report predating the `golden` tier, or one where this case
 * didn't run, simply contributes no entry.
 */
export function goldenScreenshotHistory(
  reportsNewestFirst: HealthReport[],
  pkgName: string,
  suite: string,
  name: string
): GoldenScreenshotHistoryEntry[] {
  const entries: GoldenScreenshotHistoryEntry[] = [];
  for (const report of reportsNewestFirst) {
    const pkg = report.tests?.find((t) => t.pkgName === pkgName);
    const tier = pkg?.tiers.find((t) => t.tier === 'golden');
    const testCase = tier?.cases?.find(
      (c) => c.suite === suite && c.name === name
    );
    if (testCase) entries.push({ generatedAt: report.generatedAt, testCase });
  }
  return entries;
}

export type LogLevelCount = { level: string; count: number };

/** Counts `recentLogLines` by (lower-cased) level, most-frequent first. */
export function logLevelCounts(lines: LogLine[]): LogLevelCount[] {
  const counts = new Map<string, number>();
  for (const line of lines) {
    const level = line.level.toLowerCase();
    counts.set(level, (counts.get(level) ?? 0) + 1);
  }
  return [...counts.entries()]
    .map(([level, count]) => ({ level, count }))
    .sort((a, b) => b.count - a.count);
}
