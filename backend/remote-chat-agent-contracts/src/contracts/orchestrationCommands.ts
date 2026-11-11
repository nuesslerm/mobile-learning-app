// backend/remote-chat-agent-contracts/src/contracts/orchestrationCommands.ts
//
// Client-issued orchestration commands, the WS stream items that carry the
// resulting state back, and the receipt a dispatch returns. Ported from
// references/t3code/packages/contracts/src/orchestration.ts (Effect Schema
// → zod 4) per docs/archive/README.md. Every command carries a
// `commandId` so the receipt and the events it produced can be correlated
// with the request that caused them.
import { z } from 'zod';
import {
  ApprovalRequestId,
  CommandId,
  IsoDateTime,
  MessageId,
  ThreadId,
  TurnId,
} from './ids.js';
import {
  InteractionModeSchema,
  ModelSelectionSchema,
  OrchestrationThreadSchema,
  OrchestrationThreadSummarySchema,
  ProviderApprovalDecisionSchema,
  RuntimeModeSchema,
} from './orchestrationCore.js';
import { OrchestrationEventSchema } from './orchestrationEvents.js';

const trimmedNonEmptyString = z.string().min(1);

export const CommandBaseSchema = z.object({
  commandId: CommandId,
  threadId: ThreadId,
});
export type CommandBase = z.infer<typeof CommandBaseSchema>;

export const ThreadCreateCommandSchema = z.object({
  ...CommandBaseSchema.shape,
  type: z.literal('thread.create'),
  title: trimmedNonEmptyString,
  cwd: trimmedNonEmptyString.optional(),
  modelSelection: ModelSelectionSchema,
  runtimeMode: RuntimeModeSchema,
  interactionMode: InteractionModeSchema,
  branch: trimmedNonEmptyString.optional(),
  worktreePath: trimmedNonEmptyString.optional(),
  createdAt: IsoDateTime,
});
export type ThreadCreateCommand = z.infer<typeof ThreadCreateCommandSchema>;

export const ThreadDeleteCommandSchema = z.object({
  ...CommandBaseSchema.shape,
  type: z.literal('thread.delete'),
});
export type ThreadDeleteCommand = z.infer<typeof ThreadDeleteCommandSchema>;

export const ThreadArchiveCommandSchema = z.object({
  ...CommandBaseSchema.shape,
  type: z.literal('thread.archive'),
});
export type ThreadArchiveCommand = z.infer<typeof ThreadArchiveCommandSchema>;

export const ThreadUnarchiveCommandSchema = z.object({
  ...CommandBaseSchema.shape,
  type: z.literal('thread.unarchive'),
});
export type ThreadUnarchiveCommand = z.infer<typeof ThreadUnarchiveCommandSchema>;

export const ThreadMetaUpdateCommandSchema = z.object({
  ...CommandBaseSchema.shape,
  type: z.literal('thread.meta.update'),
  title: trimmedNonEmptyString.optional(),
  modelSelection: ModelSelectionSchema.optional(),
  cwd: trimmedNonEmptyString.optional(),
  branch: trimmedNonEmptyString.optional(),
  worktreePath: trimmedNonEmptyString.optional(),
});
export type ThreadMetaUpdateCommand = z.infer<typeof ThreadMetaUpdateCommandSchema>;

export const ThreadRuntimeModeSetCommandSchema = z.object({
  ...CommandBaseSchema.shape,
  type: z.literal('thread.runtime-mode.set'),
  runtimeMode: RuntimeModeSchema,
  createdAt: IsoDateTime,
});
export type ThreadRuntimeModeSetCommand = z.infer<typeof ThreadRuntimeModeSetCommandSchema>;

export const ThreadInteractionModeSetCommandSchema = z.object({
  ...CommandBaseSchema.shape,
  type: z.literal('thread.interaction-mode.set'),
  interactionMode: InteractionModeSchema,
  createdAt: IsoDateTime,
});
export type ThreadInteractionModeSetCommand = z.infer<typeof ThreadInteractionModeSetCommandSchema>;

export const ThreadTurnStartMessageSchema = z.object({
  messageId: MessageId,
  role: z.literal('user'),
  text: z.string(),
  attachments: z.array(z.unknown()).optional(),
});
export type ThreadTurnStartMessage = z.infer<typeof ThreadTurnStartMessageSchema>;

export const ThreadTurnStartBootstrapCreateThreadSchema = z.object({
  title: trimmedNonEmptyString,
  cwd: trimmedNonEmptyString.optional(),
  modelSelection: ModelSelectionSchema,
  runtimeMode: RuntimeModeSchema,
  interactionMode: InteractionModeSchema,
});
export type ThreadTurnStartBootstrapCreateThread = z.infer<
  typeof ThreadTurnStartBootstrapCreateThreadSchema
>;

export const ThreadTurnStartBootstrapSchema = z.object({
  createThread: ThreadTurnStartBootstrapCreateThreadSchema,
});
export type ThreadTurnStartBootstrap = z.infer<typeof ThreadTurnStartBootstrapSchema>;

export const ThreadTurnStartCommandSchema = z.object({
  ...CommandBaseSchema.shape,
  type: z.literal('thread.turn.start'),
  message: ThreadTurnStartMessageSchema,
  modelSelection: ModelSelectionSchema.optional(),
  runtimeMode: RuntimeModeSchema,
  interactionMode: InteractionModeSchema,
  sourceProposedPlan: z.unknown().optional(),
  bootstrap: ThreadTurnStartBootstrapSchema.optional(),
  createdAt: IsoDateTime,
});
export type ThreadTurnStartCommand = z.infer<typeof ThreadTurnStartCommandSchema>;

export const ThreadTurnInterruptCommandSchema = z.object({
  ...CommandBaseSchema.shape,
  type: z.literal('thread.turn.interrupt'),
  turnId: TurnId.optional(),
  createdAt: IsoDateTime,
});
export type ThreadTurnInterruptCommand = z.infer<typeof ThreadTurnInterruptCommandSchema>;

export const ThreadApprovalRespondCommandSchema = z.object({
  ...CommandBaseSchema.shape,
  type: z.literal('thread.approval.respond'),
  requestId: ApprovalRequestId,
  decision: ProviderApprovalDecisionSchema,
  createdAt: IsoDateTime,
});
export type ThreadApprovalRespondCommand = z.infer<typeof ThreadApprovalRespondCommandSchema>;

export const ThreadUserInputRespondCommandSchema = z.object({
  ...CommandBaseSchema.shape,
  type: z.literal('thread.user-input.respond'),
  requestId: ApprovalRequestId,
  answers: z.record(z.string(), z.unknown()),
  createdAt: IsoDateTime,
});
export type ThreadUserInputRespondCommand = z.infer<typeof ThreadUserInputRespondCommandSchema>;

export const ThreadSessionStopCommandSchema = z.object({
  ...CommandBaseSchema.shape,
  type: z.literal('thread.session.stop'),
  createdAt: IsoDateTime,
});
export type ThreadSessionStopCommand = z.infer<typeof ThreadSessionStopCommandSchema>;

export const ClientOrchestrationCommandSchema = z.discriminatedUnion('type', [
  ThreadCreateCommandSchema,
  ThreadDeleteCommandSchema,
  ThreadArchiveCommandSchema,
  ThreadUnarchiveCommandSchema,
  ThreadMetaUpdateCommandSchema,
  ThreadRuntimeModeSetCommandSchema,
  ThreadInteractionModeSetCommandSchema,
  ThreadTurnStartCommandSchema,
  ThreadTurnInterruptCommandSchema,
  ThreadApprovalRespondCommandSchema,
  ThreadUserInputRespondCommandSchema,
  ThreadSessionStopCommandSchema,
]);
export type ClientOrchestrationCommand = z.infer<typeof ClientOrchestrationCommandSchema>;
export type ClientOrchestrationCommandType = ClientOrchestrationCommand['type'];

export const OrchestrationThreadStreamItemSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('synchronized') }),
  z.object({ kind: z.literal('snapshot'), snapshot: OrchestrationThreadSchema }),
  z.object({ kind: z.literal('event'), event: OrchestrationEventSchema }),
]);
export type OrchestrationThreadStreamItem = z.infer<typeof OrchestrationThreadStreamItemSchema>;

export const OrchestrationShellStreamItemSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('synchronized') }),
  z.object({
    kind: z.literal('snapshot'),
    snapshot: z.array(OrchestrationThreadSummarySchema),
  }),
  z.object({ kind: z.literal('event'), event: OrchestrationEventSchema }),
]);
export type OrchestrationShellStreamItem = z.infer<typeof OrchestrationShellStreamItemSchema>;

export const OrchestrationCommandReceiptSchema = z.object({
  commandId: CommandId,
  status: z.enum(['accepted', 'rejected']),
  events: z.array(OrchestrationEventSchema),
  error: z
    .object({
      code: trimmedNonEmptyString,
      message: z.string(),
    })
    .optional(),
});
export type OrchestrationCommandReceipt = z.infer<typeof OrchestrationCommandReceiptSchema>;
