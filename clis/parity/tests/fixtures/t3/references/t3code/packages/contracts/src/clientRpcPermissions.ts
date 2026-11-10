import { AuthOrchestrationOperateScope, AuthSourceControlWriteScope } from "./auth.ts";
import { WS_METHODS } from "./rpc.ts";

export const CLIENT_GUARDED_RPC_SCOPES = {
  [WS_METHODS.reachedWrite]: AuthSourceControlWriteScope,
  [WS_METHODS.reachedProbe]: AuthOrchestrationOperateScope,
};
