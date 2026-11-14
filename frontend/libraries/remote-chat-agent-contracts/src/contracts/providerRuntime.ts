// frontend/libraries/remote-chat-agent-contracts/src/contracts/providerRuntime.ts
//
// Provider runtime event union, ported from
// references/t3code/packages/contracts/src/providerRuntime.ts (Effect Schema
// → zod 4). Discriminated by `type`; every event shares
// ProviderRuntimeEventBase (eventId, provider, threadId, createdAt, optional
// turnId/itemId/requestId/raw). The provider list is the closed trio
// (opencode, claude, codex); providerInstanceId is intentionally absent —
// instance routing is a backend/remote-chat-agent concern, not part of the
// runtime contract.
import { z } from 'zod';
import { EventId, ProviderItemId, ThreadId, TurnId } from './ids.js';

export const ProviderName = z.enum(['opencode', 'claude', 'codex']);
export type ProviderName = z.infer<typeof ProviderName>;

export const ProviderRuntimeEventRawSchema = z.object({
  source: z.string(),
  method: z.string().optional(),
  messageType: z.string().optional(),
  payload: z.unknown(),
});
export type ProviderRuntimeEventRaw = z.infer<typeof ProviderRuntimeEventRawSchema>;

const trimmedNonEmptyString = z.string().min(1);

export const RuntimeSessionState = z.enum([
  'starting',
  'ready',
  'running',
  'waiting',
  'stopped',
  'error',
]);
export type RuntimeSessionState = z.infer<typeof RuntimeSessionState>;

export const RuntimeThreadState = z.enum([
  'active',
  'idle',
  'archived',
  'closed',
  'compacted',
  'error',
]);
export type RuntimeThreadState = z.infer<typeof RuntimeThreadState>;

export const RuntimeTurnState = z.enum(['completed', 'failed', 'interrupted', 'cancelled']);
export type RuntimeTurnState = z.infer<typeof RuntimeTurnState>;

export const RuntimePlanStepStatus = z.enum(['pending', 'inProgress', 'completed']);
export type RuntimePlanStepStatus = z.infer<typeof RuntimePlanStepStatus>;

export const RuntimeItemStatus = z.enum(['inProgress', 'completed', 'failed', 'declined']);
export type RuntimeItemStatus = z.infer<typeof RuntimeItemStatus>;

export const RuntimeContentStreamKind = z.enum([
  'assistant_text',
  'reasoning_text',
  'reasoning_summary_text',
  'plan_text',
  'command_output',
  'file_change_output',
  'unknown',
]);
export type RuntimeContentStreamKind = z.infer<typeof RuntimeContentStreamKind>;

export const RuntimeSessionExitKind = z.enum(['graceful', 'error']);
export type RuntimeSessionExitKind = z.infer<typeof RuntimeSessionExitKind>;

export const RuntimeErrorClass = z.enum([
  'provider_error',
  'transport_error',
  'permission_error',
  'validation_error',
  'unknown',
]);
export type RuntimeErrorClass = z.infer<typeof RuntimeErrorClass>;

export const ToolLifecycleItemType = z.enum([
  'command_execution',
  'file_change',
  'mcp_tool_call',
  'dynamic_tool_call',
  'collab_agent_tool_call',
  'web_search',
  'image_view',
]);
export type ToolLifecycleItemType = z.infer<typeof ToolLifecycleItemType>;

export const CanonicalItemType = z.enum([
  'user_message',
  'assistant_message',
  'reasoning',
  'plan',
  'command_execution',
  'file_change',
  'mcp_tool_call',
  'dynamic_tool_call',
  'collab_agent_tool_call',
  'web_search',
  'image_view',
  'review_entered',
  'review_exited',
  'context_compaction',
  'error',
  'unknown',
]);
export type CanonicalItemType = z.infer<typeof CanonicalItemType>;

export const CanonicalRequestType = z.enum([
  'command_execution_approval',
  'file_read_approval',
  'file_change_approval',
  'apply_patch_approval',
  'exec_command_approval',
  'tool_user_input',
  'dynamic_tool_call',
  'auth_tokens_refresh',
  'unknown',
]);
export type CanonicalRequestType = z.infer<typeof CanonicalRequestType>;

export const ProviderRuntimeEventBaseSchema = z.object({
  eventId: EventId,
  provider: ProviderName,
  threadId: ThreadId,
  createdAt: z.string().datetime(),
  turnId: TurnId.optional(),
  itemId: ProviderItemId.optional(),
  requestId: trimmedNonEmptyString.optional(),
  raw: ProviderRuntimeEventRawSchema.optional(),
});
export type ProviderRuntimeEventBase = z.infer<typeof ProviderRuntimeEventBaseSchema>;

export const SessionStartedPayloadSchema = z.object({
  message: trimmedNonEmptyString.optional(),
  resume: z.unknown().optional(),
});
export type SessionStartedPayload = z.infer<typeof SessionStartedPayloadSchema>;

export const SessionStateChangedPayloadSchema = z.object({
  state: RuntimeSessionState,
  reason: trimmedNonEmptyString.optional(),
  detail: z.unknown().optional(),
});
export type SessionStateChangedPayload = z.infer<typeof SessionStateChangedPayloadSchema>;

export const SessionExitedPayloadSchema = z.object({
  reason: trimmedNonEmptyString.optional(),
  recoverable: z.boolean().optional(),
  exitKind: RuntimeSessionExitKind.optional(),
});
export type SessionExitedPayload = z.infer<typeof SessionExitedPayloadSchema>;

export const ThreadStartedPayloadSchema = z.object({
  providerThreadId: trimmedNonEmptyString.optional(),
});
export type ThreadStartedPayload = z.infer<typeof ThreadStartedPayloadSchema>;

export const ThreadStateChangedPayloadSchema = z.object({
  state: RuntimeThreadState,
  detail: z.unknown().optional(),
});
export type ThreadStateChangedPayload = z.infer<typeof ThreadStateChangedPayloadSchema>;

export const ThreadMetadataUpdatedPayloadSchema = z.object({
  name: trimmedNonEmptyString.optional(),
  metadata: z.record(z.string(), z.unknown()).optional(),
});
export type ThreadMetadataUpdatedPayload = z.infer<typeof ThreadMetadataUpdatedPayloadSchema>;

export const ThreadTokenUsageSnapshotSchema = z.object({
  usedTokens: z.int().nonnegative(),
  totalProcessedTokens: z.int().nonnegative().optional(),
  maxTokens: z.int().positive().optional(),
  inputTokens: z.int().nonnegative().optional(),
  cachedInputTokens: z.int().nonnegative().optional(),
  outputTokens: z.int().nonnegative().optional(),
  reasoningOutputTokens: z.int().nonnegative().optional(),
  lastUsedTokens: z.int().nonnegative().optional(),
  lastInputTokens: z.int().nonnegative().optional(),
  lastCachedInputTokens: z.int().nonnegative().optional(),
  lastOutputTokens: z.int().nonnegative().optional(),
  lastReasoningOutputTokens: z.int().nonnegative().optional(),
  toolUses: z.int().nonnegative().optional(),
  durationMs: z.int().nonnegative().optional(),
  compactsAutomatically: z.boolean().optional(),
});
export type ThreadTokenUsageSnapshot = z.infer<typeof ThreadTokenUsageSnapshotSchema>;

export const ThreadTokenUsageUpdatedPayloadSchema = z.object({
  usage: ThreadTokenUsageSnapshotSchema,
});
export type ThreadTokenUsageUpdatedPayload = z.infer<typeof ThreadTokenUsageUpdatedPayloadSchema>;

export const TurnStartedPayloadSchema = z.object({
  model: trimmedNonEmptyString.optional(),
  effort: trimmedNonEmptyString.optional(),
});
export type TurnStartedPayload = z.infer<typeof TurnStartedPayloadSchema>;

export const TurnCompletedPayloadSchema = z.object({
  state: RuntimeTurnState,
  stopReason: trimmedNonEmptyString.nullable().optional(),
  usage: z.unknown().optional(),
  modelUsage: z.record(z.string(), z.unknown()).optional(),
  totalCostUsd: z.number().optional(),
  errorMessage: trimmedNonEmptyString.optional(),
});
export type TurnCompletedPayload = z.infer<typeof TurnCompletedPayloadSchema>;

export const TurnAbortedPayloadSchema = z.object({
  reason: trimmedNonEmptyString,
});
export type TurnAbortedPayload = z.infer<typeof TurnAbortedPayloadSchema>;

export const RuntimePlanStepSchema = z.object({
  step: trimmedNonEmptyString,
  status: RuntimePlanStepStatus,
});
export type RuntimePlanStep = z.infer<typeof RuntimePlanStepSchema>;

export const TurnPlanUpdatedPayloadSchema = z.object({
  explanation: trimmedNonEmptyString.nullable().optional(),
  plan: z.array(RuntimePlanStepSchema),
});
export type TurnPlanUpdatedPayload = z.infer<typeof TurnPlanUpdatedPayloadSchema>;

export const TurnProposedDeltaPayloadSchema = z.object({
  delta: z.string(),
});
export type TurnProposedDeltaPayload = z.infer<typeof TurnProposedDeltaPayloadSchema>;

export const TurnProposedCompletedPayloadSchema = z.object({
  planMarkdown: trimmedNonEmptyString,
});
export type TurnProposedCompletedPayload = z.infer<typeof TurnProposedCompletedPayloadSchema>;

export const TurnDiffUpdatedPayloadSchema = z.object({
  unifiedDiff: z.string(),
});
export type TurnDiffUpdatedPayload = z.infer<typeof TurnDiffUpdatedPayloadSchema>;

export const ItemLifecyclePayloadSchema = z.object({
  itemType: CanonicalItemType,
  status: RuntimeItemStatus.optional(),
  title: trimmedNonEmptyString.optional(),
  detail: trimmedNonEmptyString.optional(),
  data: z.unknown().optional(),
  agentId: trimmedNonEmptyString.optional(),
  parentToolUseId: trimmedNonEmptyString.optional(),
});
export type ItemLifecyclePayload = z.infer<typeof ItemLifecyclePayloadSchema>;

export const ContentDeltaPayloadSchema = z.object({
  streamKind: RuntimeContentStreamKind,
  delta: z.string(),
  contentIndex: z.int().optional(),
  summaryIndex: z.int().optional(),
});
export type ContentDeltaPayload = z.infer<typeof ContentDeltaPayloadSchema>;

export const RequestOpenedPayloadSchema = z.object({
  requestType: CanonicalRequestType,
  detail: trimmedNonEmptyString.optional(),
  args: z.unknown().optional(),
});
export type RequestOpenedPayload = z.infer<typeof RequestOpenedPayloadSchema>;

export const RequestResolvedPayloadSchema = z.object({
  requestType: CanonicalRequestType,
  decision: trimmedNonEmptyString.optional(),
  resolution: z.unknown().optional(),
});
export type RequestResolvedPayload = z.infer<typeof RequestResolvedPayloadSchema>;

export const UserInputQuestionOptionSchema = z.object({
  label: trimmedNonEmptyString,
  description: trimmedNonEmptyString,
});
export type UserInputQuestionOption = z.infer<typeof UserInputQuestionOptionSchema>;

export const UserInputQuestionSchema = z.object({
  id: trimmedNonEmptyString,
  header: trimmedNonEmptyString,
  question: trimmedNonEmptyString,
  options: z.array(UserInputQuestionOptionSchema),
  multiSelect: z.boolean().default(false),
});
export type UserInputQuestion = z.infer<typeof UserInputQuestionSchema>;

export const UserInputRequestedPayloadSchema = z.object({
  questions: z.array(UserInputQuestionSchema),
});
export type UserInputRequestedPayload = z.infer<typeof UserInputRequestedPayloadSchema>;

export const UserInputResolvedPayloadSchema = z.object({
  answers: z.record(z.string(), z.unknown()),
});
export type UserInputResolvedPayload = z.infer<typeof UserInputResolvedPayloadSchema>;

export const ModelReroutedPayloadSchema = z.object({
  fromModel: trimmedNonEmptyString,
  toModel: trimmedNonEmptyString,
  reason: trimmedNonEmptyString,
});
export type ModelReroutedPayload = z.infer<typeof ModelReroutedPayloadSchema>;

export const ToolDeniedPayloadSchema = z.object({
  toolName: trimmedNonEmptyString,
  toolUseId: trimmedNonEmptyString.optional(),
  reason: trimmedNonEmptyString.optional(),
  agentId: trimmedNonEmptyString.optional(),
});
export type ToolDeniedPayload = z.infer<typeof ToolDeniedPayloadSchema>;

export const RuntimeWarningPayloadSchema = z.object({
  message: trimmedNonEmptyString,
  detail: z.unknown().optional(),
});
export type RuntimeWarningPayload = z.infer<typeof RuntimeWarningPayloadSchema>;

export const RuntimeErrorPayloadSchema = z.object({
  message: trimmedNonEmptyString,
  class: RuntimeErrorClass.optional(),
  detail: z.unknown().optional(),
});
export type RuntimeErrorPayload = z.infer<typeof RuntimeErrorPayloadSchema>;

export const ProviderRuntimeEventSchema = z.discriminatedUnion('type', [
  z.object({
    ...ProviderRuntimeEventBaseSchema.shape,
    type: z.literal('session.started'),
    payload: SessionStartedPayloadSchema,
  }),
  z.object({
    ...ProviderRuntimeEventBaseSchema.shape,
    type: z.literal('session.state.changed'),
    payload: SessionStateChangedPayloadSchema,
  }),
  z.object({
    ...ProviderRuntimeEventBaseSchema.shape,
    type: z.literal('session.exited'),
    payload: SessionExitedPayloadSchema,
  }),
  z.object({
    ...ProviderRuntimeEventBaseSchema.shape,
    type: z.literal('thread.started'),
    payload: ThreadStartedPayloadSchema,
  }),
  z.object({
    ...ProviderRuntimeEventBaseSchema.shape,
    type: z.literal('thread.state.changed'),
    payload: ThreadStateChangedPayloadSchema,
  }),
  z.object({
    ...ProviderRuntimeEventBaseSchema.shape,
    type: z.literal('thread.metadata.updated'),
    payload: ThreadMetadataUpdatedPayloadSchema,
  }),
  z.object({
    ...ProviderRuntimeEventBaseSchema.shape,
    type: z.literal('thread.token-usage.updated'),
    payload: ThreadTokenUsageUpdatedPayloadSchema,
  }),
  z.object({
    ...ProviderRuntimeEventBaseSchema.shape,
    type: z.literal('turn.started'),
    payload: TurnStartedPayloadSchema,
  }),
  z.object({
    ...ProviderRuntimeEventBaseSchema.shape,
    type: z.literal('turn.completed'),
    payload: TurnCompletedPayloadSchema,
  }),
  z.object({
    ...ProviderRuntimeEventBaseSchema.shape,
    type: z.literal('turn.aborted'),
    payload: TurnAbortedPayloadSchema,
  }),
  z.object({
    ...ProviderRuntimeEventBaseSchema.shape,
    type: z.literal('turn.plan.updated'),
    payload: TurnPlanUpdatedPayloadSchema,
  }),
  z.object({
    ...ProviderRuntimeEventBaseSchema.shape,
    type: z.literal('turn.proposed.delta'),
    payload: TurnProposedDeltaPayloadSchema,
  }),
  z.object({
    ...ProviderRuntimeEventBaseSchema.shape,
    type: z.literal('turn.proposed.completed'),
    payload: TurnProposedCompletedPayloadSchema,
  }),
  z.object({
    ...ProviderRuntimeEventBaseSchema.shape,
    type: z.literal('turn.diff.updated'),
    payload: TurnDiffUpdatedPayloadSchema,
  }),
  z.object({
    ...ProviderRuntimeEventBaseSchema.shape,
    type: z.literal('item.started'),
    payload: ItemLifecyclePayloadSchema,
  }),
  z.object({
    ...ProviderRuntimeEventBaseSchema.shape,
    type: z.literal('item.updated'),
    payload: ItemLifecyclePayloadSchema,
  }),
  z.object({
    ...ProviderRuntimeEventBaseSchema.shape,
    type: z.literal('item.completed'),
    payload: ItemLifecyclePayloadSchema,
  }),
  z.object({
    ...ProviderRuntimeEventBaseSchema.shape,
    type: z.literal('content.delta'),
    payload: ContentDeltaPayloadSchema,
  }),
  z.object({
    ...ProviderRuntimeEventBaseSchema.shape,
    type: z.literal('request.opened'),
    payload: RequestOpenedPayloadSchema,
  }),
  z.object({
    ...ProviderRuntimeEventBaseSchema.shape,
    type: z.literal('request.resolved'),
    payload: RequestResolvedPayloadSchema,
  }),
  z.object({
    ...ProviderRuntimeEventBaseSchema.shape,
    type: z.literal('user-input.requested'),
    payload: UserInputRequestedPayloadSchema,
  }),
  z.object({
    ...ProviderRuntimeEventBaseSchema.shape,
    type: z.literal('user-input.resolved'),
    payload: UserInputResolvedPayloadSchema,
  }),
  z.object({
    ...ProviderRuntimeEventBaseSchema.shape,
    type: z.literal('tool.denied'),
    payload: ToolDeniedPayloadSchema,
  }),
  z.object({
    ...ProviderRuntimeEventBaseSchema.shape,
    type: z.literal('runtime.warning'),
    payload: RuntimeWarningPayloadSchema,
  }),
  z.object({
    ...ProviderRuntimeEventBaseSchema.shape,
    type: z.literal('runtime.error'),
    payload: RuntimeErrorPayloadSchema,
  }),
  z.object({
    ...ProviderRuntimeEventBaseSchema.shape,
    type: z.literal('model.rerouted'),
    payload: ModelReroutedPayloadSchema,
  }),
]);
export type ProviderRuntimeEvent = z.infer<typeof ProviderRuntimeEventSchema>;
