// backend/remote-chat-agent-contracts/src/contracts/orchestrationEvents.ts
//
// Discriminated union of orchestration events. Every event extends the same
// base (eventId, sequence, createdAt, threadId, optional commandId, actor)
// and carries a payload typed by `type`. The union is the only source of
// truth for what an orchestration event looks like on the wire; the
// projector in backend/remote-chat-agent consumes it to maintain the
// OrchestrationReadModel. Payloads are intentionally minimal — only what
// the decider needs to mutate the read model — and ported from
// references/t3code/packages/contracts/src/orchestration.ts (Effect Schema
// → zod 4) per docs/archive/plans/coding-agent/contract.md.
import { z } from 'zod';
import {
  ApprovalRequestId,
  CommandId,
  EventId,
  IsoDateTime,
  MessageId,
  ThreadId,
  TurnId,
} from './ids.js';
import {
  InteractionModeSchema,
  ModelSelectionSchema,
  OrchestrationMessageSchema,
  OrchestrationProposedPlanSchema,
  OrchestrationSessionSchema,
  OrchestrationThreadActivitySchema,
  ProviderApprovalDecisionSchema,
  RuntimeModeSchema,
} from './orchestrationCore.js';

const trimmedNonEmptyString = z.string().min(1);

export const OrchestrationEventActorSchema = z.enum(['client', 'provider', 'system']);
export type OrchestrationEventActor = z.infer<typeof OrchestrationEventActorSchema>;

export const EventBaseSchema = z.object({
  eventId: EventId,
  sequence: z.number().int(),
  createdAt: IsoDateTime,
  threadId: ThreadId,
  commandId: CommandId.optional(),
  actor: OrchestrationEventActorSchema,
});
export type EventBase = z.infer<typeof EventBaseSchema>;

export const ThreadCreatedPayloadSchema = z.object({
  title: trimmedNonEmptyString,
  cwd: trimmedNonEmptyString,
  modelSelection: ModelSelectionSchema,
  runtimeMode: RuntimeModeSchema,
  interactionMode: InteractionModeSchema,
  branch: trimmedNonEmptyString.optional(),
  worktreePath: trimmedNonEmptyString.optional(),
  createdAt: IsoDateTime,
});
export type ThreadCreatedPayload = z.infer<typeof ThreadCreatedPayloadSchema>;

export const ThreadDeletedPayloadSchema = z.object({
  deletedAt: IsoDateTime,
});
export type ThreadDeletedPayload = z.infer<typeof ThreadDeletedPayloadSchema>;

export const ThreadArchivedPayloadSchema = z.object({
  archivedAt: IsoDateTime,
  updatedAt: IsoDateTime,
});
export type ThreadArchivedPayload = z.infer<typeof ThreadArchivedPayloadSchema>;

export const ThreadUnarchivedPayloadSchema = z.object({
  updatedAt: IsoDateTime,
});
export type ThreadUnarchivedPayload = z.infer<typeof ThreadUnarchivedPayloadSchema>;

export const ThreadMetaUpdatedPayloadSchema = z.object({
  title: trimmedNonEmptyString.optional(),
  modelSelection: ModelSelectionSchema.optional(),
  cwd: trimmedNonEmptyString.optional(),
  branch: trimmedNonEmptyString.optional(),
  worktreePath: trimmedNonEmptyString.optional(),
  updatedAt: IsoDateTime,
});
export type ThreadMetaUpdatedPayload = z.infer<typeof ThreadMetaUpdatedPayloadSchema>;

export const ThreadRuntimeModeSetPayloadSchema = z.object({
  runtimeMode: RuntimeModeSchema,
  updatedAt: IsoDateTime,
});
export type ThreadRuntimeModeSetPayload = z.infer<typeof ThreadRuntimeModeSetPayloadSchema>;

export const ThreadInteractionModeSetPayloadSchema = z.object({
  interactionMode: InteractionModeSchema,
  updatedAt: IsoDateTime,
});
export type ThreadInteractionModeSetPayload = z.infer<typeof ThreadInteractionModeSetPayloadSchema>;

export const ThreadMessageSentPayloadSchema = z.object({
  message: OrchestrationMessageSchema,
});
export type ThreadMessageSentPayload = z.infer<typeof ThreadMessageSentPayloadSchema>;

export const ThreadTurnStartRequestedPayloadSchema = z.object({
  messageId: MessageId,
  modelSelection: ModelSelectionSchema.optional(),
  runtimeMode: RuntimeModeSchema,
  interactionMode: InteractionModeSchema,
  sourceProposedPlan: z.unknown().optional(),
  createdAt: IsoDateTime,
});
export type ThreadTurnStartRequestedPayload = z.infer<typeof ThreadTurnStartRequestedPayloadSchema>;

export const ThreadTurnInterruptRequestedPayloadSchema = z.object({
  turnId: TurnId.optional(),
  createdAt: IsoDateTime,
});
export type ThreadTurnInterruptRequestedPayload = z.infer<typeof ThreadTurnInterruptRequestedPayloadSchema>;

export const ThreadApprovalResponseRequestedPayloadSchema = z.object({
  requestId: ApprovalRequestId,
  decision: ProviderApprovalDecisionSchema,
  createdAt: IsoDateTime,
});
export type ThreadApprovalResponseRequestedPayload = z.infer<typeof ThreadApprovalResponseRequestedPayloadSchema>;

export const ThreadUserInputResponseRequestedPayloadSchema = z.object({
  requestId: ApprovalRequestId,
  answers: z.record(z.string(), z.unknown()),
  createdAt: IsoDateTime,
});
export type ThreadUserInputResponseRequestedPayload = z.infer<typeof ThreadUserInputResponseRequestedPayloadSchema>;

export const ThreadSessionStopRequestedPayloadSchema = z.object({
  createdAt: IsoDateTime,
});
export type ThreadSessionStopRequestedPayload = z.infer<typeof ThreadSessionStopRequestedPayloadSchema>;

export const ThreadSessionSetPayloadSchema = z.object({
  session: OrchestrationSessionSchema,
});
export type ThreadSessionSetPayload = z.infer<typeof ThreadSessionSetPayloadSchema>;

export const ThreadProposedPlanUpsertedPayloadSchema = z.object({
  proposedPlan: OrchestrationProposedPlanSchema,
});
export type ThreadProposedPlanUpsertedPayload = z.infer<typeof ThreadProposedPlanUpsertedPayloadSchema>;

export const ThreadActivityAppendedPayloadSchema = z.object({
  activity: OrchestrationThreadActivitySchema,
});
export type ThreadActivityAppendedPayload = z.infer<typeof ThreadActivityAppendedPayloadSchema>;

export const ThreadTurnDiffUpdatedPayloadSchema = z.object({
  turnId: TurnId,
  unifiedDiff: z.string(),
});
export type ThreadTurnDiffUpdatedPayload = z.infer<typeof ThreadTurnDiffUpdatedPayloadSchema>;

export const OrchestrationEventSchema = z.discriminatedUnion('type', [
  z.object({
    ...EventBaseSchema.shape,
    type: z.literal('thread.created'),
    payload: ThreadCreatedPayloadSchema,
  }),
  z.object({
    ...EventBaseSchema.shape,
    type: z.literal('thread.deleted'),
    payload: ThreadDeletedPayloadSchema,
  }),
  z.object({
    ...EventBaseSchema.shape,
    type: z.literal('thread.archived'),
    payload: ThreadArchivedPayloadSchema,
  }),
  z.object({
    ...EventBaseSchema.shape,
    type: z.literal('thread.unarchived'),
    payload: ThreadUnarchivedPayloadSchema,
  }),
  z.object({
    ...EventBaseSchema.shape,
    type: z.literal('thread.meta-updated'),
    payload: ThreadMetaUpdatedPayloadSchema,
  }),
  z.object({
    ...EventBaseSchema.shape,
    type: z.literal('thread.runtime-mode-set'),
    payload: ThreadRuntimeModeSetPayloadSchema,
  }),
  z.object({
    ...EventBaseSchema.shape,
    type: z.literal('thread.interaction-mode-set'),
    payload: ThreadInteractionModeSetPayloadSchema,
  }),
  z.object({
    ...EventBaseSchema.shape,
    type: z.literal('thread.message-sent'),
    payload: ThreadMessageSentPayloadSchema,
  }),
  z.object({
    ...EventBaseSchema.shape,
    type: z.literal('thread.turn-start-requested'),
    payload: ThreadTurnStartRequestedPayloadSchema,
  }),
  z.object({
    ...EventBaseSchema.shape,
    type: z.literal('thread.turn-interrupt-requested'),
    payload: ThreadTurnInterruptRequestedPayloadSchema,
  }),
  z.object({
    ...EventBaseSchema.shape,
    type: z.literal('thread.approval-response-requested'),
    payload: ThreadApprovalResponseRequestedPayloadSchema,
  }),
  z.object({
    ...EventBaseSchema.shape,
    type: z.literal('thread.user-input-response-requested'),
    payload: ThreadUserInputResponseRequestedPayloadSchema,
  }),
  z.object({
    ...EventBaseSchema.shape,
    type: z.literal('thread.session-stop-requested'),
    payload: ThreadSessionStopRequestedPayloadSchema,
  }),
  z.object({
    ...EventBaseSchema.shape,
    type: z.literal('thread.session-set'),
    payload: ThreadSessionSetPayloadSchema,
  }),
  z.object({
    ...EventBaseSchema.shape,
    type: z.literal('thread.proposed-plan-upserted'),
    payload: ThreadProposedPlanUpsertedPayloadSchema,
  }),
  z.object({
    ...EventBaseSchema.shape,
    type: z.literal('thread.activity-appended'),
    payload: ThreadActivityAppendedPayloadSchema,
  }),
  z.object({
    ...EventBaseSchema.shape,
    type: z.literal('thread.turn-diff-updated'),
    payload: ThreadTurnDiffUpdatedPayloadSchema,
  }),
]);
export type OrchestrationEvent = z.infer<typeof OrchestrationEventSchema>;
export type OrchestrationEventType = OrchestrationEvent['type'];