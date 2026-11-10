import { ORCHESTRATION_V2_WS_METHODS } from "./orchestrationV2.ts";

export const WS_METHODS = {
  reachedWrite: "reached.write",
  reachedWatch: "reached.watch",
  reachedProbe: "reached.probe",
  reachedResolve: "reached.resolve",
  unreachedClose: "unreached.close",
  unreachedTyped: "unreached.typed",
  unreachedTest: "unreached.test",
} as const;

const WsWriteRpc = Rpc.make(WS_METHODS.reachedWrite, { payload: X });
const WsWatchRpc = Rpc.make(WS_METHODS.reachedWatch, {
  payload: X,
  stream: true,
});
const WsDispatchRpc = Rpc.make(ORCHESTRATION_V2_WS_METHODS.dispatchCommand, { payload: X });
