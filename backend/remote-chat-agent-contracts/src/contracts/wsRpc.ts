// backend/remote-chat-agent-contracts/src/contracts/wsRpc.ts
//
// Wire envelope and RPC tables for the remote-chat-agent WebSocket transport.
// Ported from references/t3code/packages/contracts/src/rpc.ts (Effect Schema
// → zod 4) per docs/archive/README.md. The outer `kind`
// discriminator splits client↔server traffic; the response variant further
// splits on `ok` so `result` and `error` never appear together. Method
// and subscription tables are typed record-of-shape lookups: the dispatch
// layer maps a string `method` to `{ params, result }` zod schemas, so
// handlers get a typed payload without re-running a hand-written switch.
import { z } from 'zod';
import { TurnId } from './ids.js';
import {
  ClientOrchestrationCommandSchema,
  OrchestrationCommandReceiptSchema,
  OrchestrationShellStreamItemSchema,
  OrchestrationThreadStreamItemSchema,
} from './orchestrationCommands.js';
import { OrchestrationThreadSchema, OrchestrationThreadSummarySchema } from './orchestrationCore.js';
import { ProviderName } from './providerRuntime.js';

export const WS_PROTOCOL_VERSION = 1;
export const WS_HEARTBEAT_INTERVAL_MS = 15000;
export const WS_PATH = '/ws';

const trimmedNonEmptyString = z.string().min(1);

export const RequestIdSchema = trimmedNonEmptyString.brand<'RequestId'>();
export type RequestId = z.infer<typeof RequestIdSchema>;

export const ServerVersionSchema = trimmedNonEmptyString;
export type ServerVersion = z.infer<typeof ServerVersionSchema>;

export const WsErrorCodeSchema = trimmedNonEmptyString;
export type WsErrorCode = z.infer<typeof WsErrorCodeSchema>;

export const WsErrorSchema = z.object({
  code: WsErrorCodeSchema,
  message: z.string(),
  retryable: z.boolean().optional(),
  meta: z.record(z.string(), z.unknown()).optional(),
});
export type WsError = z.infer<typeof WsErrorSchema>;

const ClientRequestEnvelopeBaseSchema = z.object({
  id: RequestIdSchema,
  method: z.string(),
  params: z.unknown(),
});

const ClientSubscribeEnvelopeBaseSchema = z.object({
  id: RequestIdSchema,
  method: z.string(),
  params: z.unknown(),
});

const ClientUnsubscribeEnvelopeBaseSchema = z.object({
  id: RequestIdSchema,
});

const ClientPingEnvelopeBaseSchema = z.object({
  ts: z.number().int(),
});

export const ClientEnvelopeSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('request'), ...ClientRequestEnvelopeBaseSchema.shape }),
  z.object({ kind: z.literal('subscribe'), ...ClientSubscribeEnvelopeBaseSchema.shape }),
  z.object({ kind: z.literal('unsubscribe'), ...ClientUnsubscribeEnvelopeBaseSchema.shape }),
  z.object({ kind: z.literal('ping'), ...ClientPingEnvelopeBaseSchema.shape }),
]);
export type ClientEnvelope = z.infer<typeof ClientEnvelopeSchema>;

const ServerResponseEnvelopeSchema = z
  .object({
    kind: z.literal('response'),
    id: RequestIdSchema,
    ok: z.boolean(),
    result: z.unknown().optional(),
    error: WsErrorSchema.optional(),
  })
  .refine(
    (value) =>
      value.ok
        ? value.result !== undefined && value.error === undefined
        : value.error !== undefined && value.result === undefined,
    {
      message:
        'response envelope must carry `result` when `ok: true` and `error` when `ok: false`',
    },
  );

const ServerHelloEnvelopeSchema = z.object({
  kind: z.literal('hello'),
  serverVersion: ServerVersionSchema,
  protocolVersion: z.literal(WS_PROTOCOL_VERSION),
  heartbeatIntervalMs: z.literal(WS_HEARTBEAT_INTERVAL_MS),
});

const ServerEventEnvelopeSchema = z.object({
  kind: z.literal('event'),
  id: RequestIdSchema,
  item: z.unknown(),
});

const ServerCompleteEnvelopeSchema = z.object({
  kind: z.literal('complete'),
  id: RequestIdSchema,
});

const ServerPongEnvelopeSchema = z.object({
  kind: z.literal('pong'),
  ts: z.number().int(),
});

export const ServerEnvelopeSchema = z.discriminatedUnion('kind', [
  ServerHelloEnvelopeSchema,
  ServerResponseEnvelopeSchema,
  ServerEventEnvelopeSchema,
  ServerCompleteEnvelopeSchema,
  ServerPongEnvelopeSchema,
]);
export type ServerEnvelope = z.infer<typeof ServerEnvelopeSchema>;

export const OrchestrationDispatchCommandParamsSchema = ClientOrchestrationCommandSchema;
export type OrchestrationDispatchCommandParams = z.infer<
  typeof OrchestrationDispatchCommandParamsSchema
>;

export const OrchestrationListThreadsParamsSchema = z.object({
  includeArchived: z.boolean().optional(),
});
export type OrchestrationListThreadsParams = z.infer<
  typeof OrchestrationListThreadsParamsSchema
>;

export const OrchestrationGetThreadParamsSchema = z.object({
  threadId: trimmedNonEmptyString,
});
export type OrchestrationGetThreadParams = z.infer<typeof OrchestrationGetThreadParamsSchema>;

export const OrchestrationGetTurnDiffParamsSchema = z.object({
  threadId: trimmedNonEmptyString,
  turnId: TurnId,
});
export type OrchestrationGetTurnDiffParams = z.infer<
  typeof OrchestrationGetTurnDiffParamsSchema
>;

export const OrchestrationGetTurnDiffResultSchema = z.object({
  unifiedDiff: z.string(),
});
export type OrchestrationGetTurnDiffResult = z.infer<
  typeof OrchestrationGetTurnDiffResultSchema
>;

export const ServerProbeResultProviderModelSchema = z.object({
  providerID: trimmedNonEmptyString,
  modelID: trimmedNonEmptyString,
  name: trimmedNonEmptyString,
  isDefault: z.boolean().optional(),
});
export type ServerProbeResultProviderModel = z.infer<
  typeof ServerProbeResultProviderModelSchema
>;

export const ServerProbeResultProviderSchema = z.object({
  cli: ProviderName,
  displayName: trimmedNonEmptyString,
  available: z.boolean(),
  models: z.array(ServerProbeResultProviderModelSchema),
});
export type ServerProbeResultProvider = z.infer<typeof ServerProbeResultProviderSchema>;

export const ServerProbeResultSchema = z.object({
  availableClis: z.array(ProviderName),
  selectedCli: ProviderName.nullable(),
  version: trimmedNonEmptyString,
  providers: z.array(ServerProbeResultProviderSchema),
});
export type ServerProbeResult = z.infer<typeof ServerProbeResultSchema>;

export const ModelsListParamsSchema = z.object({
  cli: ProviderName,
  directory: trimmedNonEmptyString,
});
export type ModelsListParams = z.infer<typeof ModelsListParamsSchema>;

export const ModelsListResultSchema = z.object({
  models: z.array(ServerProbeResultProviderModelSchema),
});
export type ModelsListResult = z.infer<typeof ModelsListResultSchema>;

export const OrchestrationDispatchCommandResultSchema = OrchestrationCommandReceiptSchema;
export type OrchestrationDispatchCommandResult = z.infer<
  typeof OrchestrationDispatchCommandResultSchema
>;

export const OrchestrationListThreadsResultSchema = z.object({
  threads: z.array(OrchestrationThreadSummarySchema),
});
export type OrchestrationListThreadsResult = z.infer<
  typeof OrchestrationListThreadsResultSchema
>;

export const OrchestrationGetThreadResultSchema = OrchestrationThreadSchema;
export type OrchestrationGetThreadResult = z.infer<typeof OrchestrationGetThreadResultSchema>;

export const WS_METHODS = {
  'orchestration.dispatchCommand': {
    params: OrchestrationDispatchCommandParamsSchema,
    result: OrchestrationDispatchCommandResultSchema,
  },
  'orchestration.listThreads': {
    params: OrchestrationListThreadsParamsSchema,
    result: OrchestrationListThreadsResultSchema,
  },
  'orchestration.getThread': {
    params: OrchestrationGetThreadParamsSchema,
    result: OrchestrationGetThreadResultSchema,
  },
  'orchestration.getTurnDiff': {
    params: OrchestrationGetTurnDiffParamsSchema,
    result: OrchestrationGetTurnDiffResultSchema,
  },
  'server.probe': {
    params: z.object({}),
    result: ServerProbeResultSchema,
  },
  'models.list': {
    params: ModelsListParamsSchema,
    result: ModelsListResultSchema,
  },
} as const satisfies Record<string, { params: z.ZodTypeAny; result: z.ZodTypeAny }>;
export type WsMethodTable = typeof WS_METHODS;
export type WsMethod = keyof WsMethodTable;
export const WsMethodSchema = z.enum(
  Object.keys(WS_METHODS) as [WsMethod, ...WsMethod[]],
);

export const OrchestrationSubscribeShellParamsSchema = z.object({});
export type OrchestrationSubscribeShellParams = z.infer<
  typeof OrchestrationSubscribeShellParamsSchema
>;

export const OrchestrationSubscribeThreadParamsSchema = z.object({
  threadId: trimmedNonEmptyString,
  afterSequence: z.number().int().nonnegative().optional(),
});
export type OrchestrationSubscribeThreadParams = z.infer<
  typeof OrchestrationSubscribeThreadParamsSchema
>;

export const OrchestrationSubscribeShellItemSchema = OrchestrationShellStreamItemSchema;
export type OrchestrationSubscribeShellItem = z.infer<
  typeof OrchestrationSubscribeShellItemSchema
>;

export const OrchestrationSubscribeThreadItemSchema = OrchestrationThreadStreamItemSchema;
export type OrchestrationSubscribeThreadItem = z.infer<
  typeof OrchestrationSubscribeThreadItemSchema
>;

export const WS_SUBSCRIPTIONS = {
  'orchestration.subscribeShell': {
    params: OrchestrationSubscribeShellParamsSchema,
    item: OrchestrationSubscribeShellItemSchema,
  },
  'orchestration.subscribeThread': {
    params: OrchestrationSubscribeThreadParamsSchema,
    item: OrchestrationSubscribeThreadItemSchema,
  },
} as const satisfies Record<string, { params: z.ZodTypeAny; item: z.ZodTypeAny }>;
export type WsSubscriptionTable = typeof WS_SUBSCRIPTIONS;
export type WsSubscription = keyof WsSubscriptionTable;
export const WsSubscriptionSchema = z.enum(
  Object.keys(WS_SUBSCRIPTIONS) as [WsSubscription, ...WsSubscription[]],
);
