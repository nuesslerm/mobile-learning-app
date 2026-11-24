import { WS_METHODS } from "@t3tools/contracts";

export function createAtoms(runtime: Runtime) {
  const watch = createSubscription(runtime, { tag: WS_METHODS.reachedWatch });
  const resolver = RequestResolver.makeWith({
    batchKey: (entry) => entry.session,
    runAll: Effect.fnUntraced(function* (entries) {
      return yield* request(WS_METHODS.reachedResolve, { entries });
    }),
  });
  return {
    watch,
    write: createCommand(runtime, {
      label: "write",
      tag: WS_METHODS.reachedWrite,
    }),
    close: createCommand(runtime, {
      label: "close",
      tag: WS_METHODS.unreachedClose,
    }),
  };
}

export type Typed = Input<typeof WS_METHODS.unreachedTyped>;
