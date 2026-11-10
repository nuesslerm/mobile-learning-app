// backend/remote-chat-agent-contracts/src/contracts/orchestrationCore.ts
//
// Read-model shapes ported from
// references/t3code/packages/contracts/src/orchestration.ts (Effect Schema → zod 4)
// per docs/archive/plans/coding-agent/contract.md. Pure data: no events, no commands —
// the projector over `OrchestrationEventSchema` produces these shapes.
// `OrchestrationThreadSummary` is the same thread minus the heavy collections
// so the shell list can ship on every WS tick without re-uploading messages,
// activities, or proposed plans.
import { z } from 'zod';
import {
  EventId,
  IsoDateTime,
  MessageId,
  ThreadId,
  TurnId,
} from './ids.js';
import { ProviderName } from './providerRuntime.js';

const trimmedNonEmptyString = z.string().min(1);

export const RuntimeModeSchema = z.enum([
  'approval-required',
  'auto-accept-edits',
  'auto',
  'full-access',
]);
export type RuntimeMode = z.infer<typeof RuntimeModeSchema>;

export const InteractionModeSchema = z.enum(['default', 'plan']);
export type InteractionMode = z.infer<typeof InteractionModeSchema>;

export const ProviderApprovalDecisionSchema = z.enum([
  'accept',
  'acceptForSession',
  'decline',
  'cancel',
]);
export type ProviderApprovalDecision = z.infer<typeof ProviderApprovalDecisionSchema>;

export const ModelSelectionSchema = z.object({
  cli: ProviderName,
  providerID: trimmedNonEmptyString.optional(),
  modelID: trimmedNonEmptyString.optional(),
});
export type ModelSelection = z.infer<typeof ModelSelectionSchema>;

const OrchestrationMessageRoleSchema = z.enum(['user', 'assistant', 'system']);

export const OrchestrationMessageSchema = z.object({
  id: MessageId,
  role: OrchestrationMessageRoleSchema,
  text: z.string(),
  attachments: z.array(z.unknown()).optional(),
  turnId: TurnId.optional(),
  createdAt: IsoDateTime,
  updatedAt: IsoDateTime,
});
export type OrchestrationMessage = z.infer<typeof OrchestrationMessageSchema>;

export const OrchestrationProposedPlanSchema = z.object({
  id: trimmedNonEmptyString,
  turnId: TurnId.optional(),
  planMarkdown: trimmedNonEmptyString,
  createdAt: IsoDateTime,
  updatedAt: IsoDateTime,
});
export type OrchestrationProposedPlan = z.infer<typeof OrchestrationProposedPlanSchema>;

const OrchestrationSessionStatusSchema = z.enum([
  'idle',
  'starting',
  'running',
  'ready',
  'interrupted',
  'stopped',
  'error',
]);

export const OrchestrationSessionSchema = z.object({
  cli: ProviderName,
  providerSessionId: trimmedNonEmptyString.optional(),
  status: OrchestrationSessionStatusSchema,
  activeTurnId: TurnId.optional(),
  lastError: z.string().optional(),
});
export type OrchestrationSession = z.infer<typeof OrchestrationSessionSchema>;

const OrchestrationThreadActivityToneSchema = z.enum([
  'info',
  'tool',
  'approval',
  'error',
]);

export const OrchestrationThreadActivitySchema = z.object({
  id: EventId,
  tone: OrchestrationThreadActivityToneSchema,
  kind: trimmedNonEmptyString,
  summary: trimmedNonEmptyString,
  payload: z.unknown(),
  turnId: TurnId.optional(),
  sequence: z.number().int().nonnegative().optional(),
  createdAt: IsoDateTime,
});
export type OrchestrationThreadActivity = z.infer<typeof OrchestrationThreadActivitySchema>;

const OrchestrationLatestTurnStateSchema = z.enum([
  'running',
  'interrupted',
  'completed',
  'error',
]);

export const OrchestrationLatestTurnSchema = z.object({
  turnId: TurnId,
  state: OrchestrationLatestTurnStateSchema,
  requestedAt: IsoDateTime,
  startedAt: IsoDateTime.nullable(),
  completedAt: IsoDateTime.nullable(),
});
export type OrchestrationLatestTurn = z.infer<typeof OrchestrationLatestTurnSchema>;

export const OrchestrationThreadSchema = z.object({
  id: ThreadId,
  title: trimmedNonEmptyString,
  cwd: trimmedNonEmptyString,
  modelSelection: ModelSelectionSchema,
  runtimeMode: RuntimeModeSchema,
  interactionMode: InteractionModeSchema,
  latestTurn: OrchestrationLatestTurnSchema.nullable(),
  createdAt: IsoDateTime,
  updatedAt: IsoDateTime,
  archivedAt: IsoDateTime.nullable().optional(),
  deletedAt: IsoDateTime.nullable().optional(),
  messages: z.array(OrchestrationMessageSchema),
  proposedPlans: z.array(OrchestrationProposedPlanSchema),
  activities: z.array(OrchestrationThreadActivitySchema),
  session: OrchestrationSessionSchema.nullable(),
  tokenUsage: z.unknown().optional(),
  branch: trimmedNonEmptyString.optional(),
  worktreePath: trimmedNonEmptyString.optional(),
});
export type OrchestrationThread = z.infer<typeof OrchestrationThreadSchema>;

export const OrchestrationThreadSummarySchema = OrchestrationThreadSchema.omit({
  messages: true,
  activities: true,
  proposedPlans: true,
});
export type OrchestrationThreadSummary = z.infer<typeof OrchestrationThreadSummarySchema>;

export const OrchestrationReadModelSchema = z.object({
  snapshotSequence: z.number().int().nonnegative(),
  threads: z.record(z.string(), OrchestrationThreadSchema),
  updatedAt: IsoDateTime,
});
export type OrchestrationReadModel = z.infer<typeof OrchestrationReadModelSchema>;
