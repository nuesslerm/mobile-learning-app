/**
 * `HealthReport` domain models — the shape produced by `wf check --health
 * --json` (`clis/deploy/src/report_health.rs`) and published
 * to the health-reports ORDS auto-REST endpoint as
 * `{ report_json: JSON.stringify(HealthReport) }`.
 *
 * This module is pure (no React / React Native / I/O).
 *
 * Types are derived from Zod schemas (`z.infer`) so a report fetched from
 * ORDS and `JSON.parse`d can be validated at the boundary (`safeParse`)
 * instead of trusted blindly — a bad/partial row becomes a skipped row, not
 * a crash.
 */

import { z } from 'zod';

export const containerStatusSchema = z.object({
  name: z.string(),
  state: z.string(),
  status: z.string(),
  image: z.string(),
});
export type ContainerStatus = z.infer<typeof containerStatusSchema>;

export const serviceHealthEntrySchema = z.object({
  app: z.string(),
  reachable: z.boolean(),
  containers: z.array(containerStatusSchema).optional(),
  error: z.string().optional(),
});
export type ServiceHealthEntry = z.infer<typeof serviceHealthEntrySchema>;

export const vmOpsSelfHealthSchema = z.union([
  z.object({ reachable: z.literal(true), host: z.string(), time: z.string() }),
  z.object({ reachable: z.literal(false), error: z.string() }),
]);
export type VmOpsSelfHealth = z.infer<typeof vmOpsSelfHealthSchema>;

export const logSourceSchema = z.enum(['backend', 'mobile']);
export type LogSource = z.infer<typeof logSourceSchema>;

export const sourceHealthSchema = z.object({
  source: logSourceSchema,
  /** Null when the source produced no rows at all in the window. */
  newestRowIso: z.string().nullable(),
  rowCount: z.number(),
  live: z.boolean(),
  error: z.string().optional(),
});
export type SourceHealth = z.infer<typeof sourceHealthSchema>;

export const logsDoctorResultSchema = z.object({
  healthy: z.boolean(),
  sources: z.array(sourceHealthSchema),
});
export type LogsDoctorResult = z.infer<typeof logsDoctorResultSchema>;

export const recentErrorSummarySchema = z.object({
  source: logSourceSchema,
  windowHours: z.number(),
  errorCount: z.number(),
  sampleMessages: z.array(z.string()),
  error: z.string().optional(),
});
export type RecentErrorSummary = z.infer<typeof recentErrorSummarySchema>;

export const testTierSchema = z.enum([
  'unit',
  'integration',
  'contract',
  'e2e',
  /** Device flows driven against a real simulator or emulator. */
  'flow',
  /**
   * An automated Playwright proxy for ONE docs/runbooks/*.md checklist item
   * (currently only agent-feature-sanity.md) — NOT a stand-in for manual
   * runbook verification in general. A genuine runbook check is a human/agent
   * actually clicking through the deployed product, which by definition can't
   * be represented as a re-runnable, always-green suite; this tier exists
   * only because this one checklist item happens to have an automated
   * equivalent. Named `-proxy` specifically so it never reads as "runbook
   * coverage" in the UI.
   */
  'runbook-proxy',
  /**
   * Golden screenshot regression
   * — Playwright's own `toHaveScreenshot()` diffing a real device-frame +
   * mock-keyboard capture against a checked-in baseline. `failureMessage`
   * on a failed case carries Playwright's own diff-ratio text (e.g. "12345
   * pixels (ratio 0.03 of all pixels) are different") — no separate
   * diff-percentage field, since that's already a generic `TestCaseResult`
   * field every tier shares (see that type's own doc comment for why image
   * bytes aren't embedded here at all).
   */
  'golden',
]);
export type TestTier = z.infer<typeof testTierSchema>;

export const testCaseStatusSchema = z.enum(['passed', 'failed', 'skipped']);
export type TestCaseStatus = z.infer<typeof testCaseStatusSchema>;

/**
 * One test case (a vitest/Playwright `it`/`test`, or a single Maestro flow /
 * runbook checklist item) inside a `TestTierResult`. `suite` is the file or
 * nested describe-block path so cases group visually by suite in the UI.
 *
 * For the `golden` tier specifically: deliberately no baseline/actual/diff
 * image BYTES field here — only URLs. A single golden run produces ~10 PNGs
 * at tens to ~100KB each; base64-encoding them into this JSON (already
 * published wholesale to an Oracle ORDS-backed Autonomous DB row per `wf
 * report-health --publish`) would both bloat every future row by that same
 * amount forever and add ~33% encoding overhead. Instead, when a session has
 * run `wf vm creds sync` and opted into upload, the golden suite uploads each PNG to the
 * Always-Free Object Storage bucket (infra/oracle-cloud/object-storage.tf)
 * and these three fields carry the resulting public URLs — small strings,
 * cheap to store per row, letting a screenshot-history view load images by
 * URL across many historical runs instead of just the latest one.
 * `failureMessage` on a failed case still carries Playwright's own
 * diff-ratio text as the first-glance signal.
 */
export const testCaseResultSchema = z.object({
  suite: z.string(),
  name: z.string(),
  status: testCaseStatusSchema,
  durationMs: z.number().optional(),
  failureMessage: z.string().optional(),
  baselineImageUrl: z.string().optional(),
  actualImageUrl: z.string().optional(),
  diffImageUrl: z.string().optional(),
});
export type TestCaseResult = z.infer<typeof testCaseResultSchema>;

export const testTierResultSchema = z.object({
  tier: testTierSchema,
  filesPassed: z.number().optional(),
  filesTotal: z.number().optional(),
  testsPassed: z.number().optional(),
  testsTotal: z.number().optional(),
  ranOk: z.boolean(),
  raw: z.string(),
  /**
   * Per-suite/per-test-case breakdown, populated for non-unit tiers via each
   * runner's own structured reporter (vitest/Playwright `--reporter=json`) —
   * optional so a report published before this field existed still validates.
   */
  cases: z.array(testCaseResultSchema).optional(),
  /**
   * Set when this tier could not run at all in the environment that
   * generated the report (e.g. Maestro flows need a real iOS simulator or
   * Android emulator, unavailable in a Linux sandbox) — `ranOk` stays
   * `false` but this explains WHY rather than leaving a bare failure.
   */
  unavailableReason: z.string().optional(),
});
export type TestTierResult = z.infer<typeof testTierResultSchema>;

export const packageTestResultsSchema = z.object({
  pkgName: z.string(),
  tiers: z.array(testTierResultSchema),
});
export type PackageTestResults = z.infer<typeof packageTestResultsSchema>;

export const logLineSchema = z.object({
  dt: z.string(),
  level: z.string(),
  module: z.string(),
  message: z.string(),
});
export type LogLine = z.infer<typeof logLineSchema>;

export const spanLineSchema = z.object({
  dt: z.string(),
  service: z.string(),
  name: z.string(),
  durationMs: z.number(),
  statusCode: z.number().optional(),
});
export type SpanLine = z.infer<typeof spanLineSchema>;

export const healthReportSchema = z.object({
  generatedAt: z.string(),
  vmOps: vmOpsSelfHealthSchema,
  services: z.array(serviceHealthEntrySchema),
  logs: logsDoctorResultSchema,
  recentErrors: z.array(recentErrorSummarySchema),
  tests: z.array(packageTestResultsSchema).optional(),
  e2eNote: z.string().optional(),
  /**
   * Recent raw log lines (any level) from BetterStack, most-recent first —
   * optional (like `tests`/`e2eNote` above) so a report row published before
   * this field existed still validates. Callers should treat a missing value
   * as an empty array.
   */
  recentLogLines: z.array(logLineSchema).optional(),
  /**
   * Recent OpenTelemetry spans (traces) from BetterStack's `_spans`
   * collection, most-recent first — optional for the same reason as
   * `recentLogLines`: older published rows predate this field.
   */
  recentSpans: z.array(spanLineSchema).optional(),
});
export type HealthReport = z.infer<typeof healthReportSchema>;
