import { ORCHESTRATION_V2_WS_METHODS } from "@t3tools/contracts";

function threadCommand(type: "reached.archive" | "unreached.delete", input: Input) {
  return client[ORCHESTRATION_V2_WS_METHODS.dispatchCommand]({ type, threadId: input.threadId });
}

export const archiveThread = (input: Input) => threadCommand("reached.archive", input);

export const deleteThread = (input: Input) => threadCommand("unreached.delete", input);

export const readHistory = Effect.fn(function* (input: Input) {
  return yield* withAuth({
    group: "orchestration",
    request: ({ client }) => client.reachedHistory({ params: input }),
  });
});

export const readSession = Effect.fn(function* (url: string) {
  const client = yield* makeEnvironmentHttpApiGroupClient(url, "auth");
  return yield* client.reachedSession({});
});

export const mutateProject = (input: Input) => mutate({ type: "project.reached-create", ...input });

function withAuth<A>(input: { group: string; request: (r: { client: Client }) => A }) {
  return input.request({ client: makeEnvironmentHttpApiGroupClient(base, input.group) });
}
