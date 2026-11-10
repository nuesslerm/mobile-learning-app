export const ORCHESTRATION_V2_WS_METHODS = {
  dispatchCommand: "reached.dispatch",
} as const;

export const OrchestrationV2Command = Schema.Union([
  Schema.Struct({
    type: Schema.Literal("reached.archive"),
    threadId: ThreadId,
  }),
  Schema.Struct({
    type: Schema.Literal("reached.create"),
    mode: Schema.Union([Schema.Struct({ type: Schema.Literal("nested.not-a-command") })]),
  }),
  Schema.Struct({
    type: Schema.Literal("unreached.delete"),
  }),
]);
