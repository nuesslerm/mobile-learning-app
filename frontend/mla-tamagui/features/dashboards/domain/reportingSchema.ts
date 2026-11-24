// Zod schemas for the reporting service (backend/reporting-service) read API:
// metrics, headroom, plans and runs. Mirrors the server response shapes in
// backend/reporting-service/rs/src/api.

import { z } from 'zod';

const metricSampleSchema = z.object({
  ts: z.number(),
  scope: z.string(),
  name: z.string(),
  metric: z.string(),
  value: z.number(),
});
export type MetricSample = z.infer<typeof metricSampleSchema>;

export const metricsCurrentResponseSchema = z.object({
  samples: z.array(metricSampleSchema),
});
export type MetricsCurrentResponse = z.infer<typeof metricsCurrentResponseSchema>;

export const metricsSeriesResponseSchema = z.object({
  window: z.string(),
  points: z.array(metricSampleSchema),
});
export type MetricsSeriesResponse = z.infer<typeof metricsSeriesResponseSchema>;

const headroomResourceReportSchema = z.object({
  resource: z.enum(['cpu', 'memory', 'disk']),
  used: z.number(),
  ceiling: z.number(),
  remaining: z.number(),
  unit: z.enum(['ocpu', 'bytes']),
});
export type HeadroomResourceReport = z.infer<typeof headroomResourceReportSchema>;

export const headroomResponseSchema = z.object({
  resources: z.array(headroomResourceReportSchema),
});
export type HeadroomResponse = z.infer<typeof headroomResponseSchema>;

const planDayTotalSchema = z.object({
  day: z.string(),
  runs: z.number(),
  tokens: z.number(),
  cost: z.number(),
  notionalCost: z.number().nullable(),
  unpricedRuns: z.number(),
});
export type PlanDayTotal = z.infer<typeof planDayTotalSchema>;

const planSummarySchema = z.object({
  slug: z.string(),
  units: z.number(),
  runIds: z.number(),
  resolved: z.number(),
  unresolved: z.number(),
  unpricedRuns: z.number(),
  days: z.array(planDayTotalSchema),
});
export type PlanSummary = z.infer<typeof planSummarySchema>;

export const plansResponseSchema = z.object({
  plans: z.array(planSummarySchema),
});
export type PlansResponse = z.infer<typeof plansResponseSchema>;

const resolvedRunSchema = z.object({
  runId: z.string(),
  model: z.string().nullable(),
  pool: z.string().nullable(),
  cost: z.number(),
  tokens: z.number(),
  durationMs: z.number(),
  day: z.string(),
  priced: z.boolean(),
  notionalCost: z.number().nullable(),
});
export type ResolvedRun = z.infer<typeof resolvedRunSchema>;

const unitPlanReportSchema = z.object({
  id: z.string(),
  title: z.string(),
  tier: z.string().nullable(),
  status: z.string().nullable(),
  seconds: z.number().nullable(),
  opencodeUsd: z.number().nullable(),
  opencodeTokens: z.number().nullable(),
  runIds: z.array(z.string()),
  resolved: z.number(),
  unresolved: z.number(),
  runs: z.array(resolvedRunSchema),
});
export const planDetailSchema = z.object({
  slug: z.string(),
  startedAt: z.string().nullable(),
  finishedAt: z.string().nullable(),
  units: z.array(unitPlanReportSchema),
  runIds: z.number(),
  resolved: z.number(),
  unresolved: z.number(),
  unpricedRuns: z.number(),
  days: z.array(planDayTotalSchema),
});
export type PlanDetail = z.infer<typeof planDetailSchema>;

export const runReportSchema = z.object({
  runId: z.string(),
  ts: z.string(),
  day: z.string(),
  model: z.string().nullable(),
  pool: z.string().nullable(),
  cost: z.number(),
  tokens: z.number(),
  durationMs: z.number(),
  priced: z.boolean(),
  notionalCost: z.number().nullable(),
});
export type RunReport = z.infer<typeof runReportSchema>;