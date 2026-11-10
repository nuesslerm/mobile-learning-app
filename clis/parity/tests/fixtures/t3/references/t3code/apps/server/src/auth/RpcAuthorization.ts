import { AuthOrchestrationOperateScope, AuthOrchestrationReadScope, CLIENT_GUARDED_RPC_SCOPES, ORCHESTRATION_V2_WS_METHODS, WS_METHODS } from "@t3tools/contracts";

export const RPC_REQUIRED_SCOPES = {
  ...CLIENT_GUARDED_RPC_SCOPES,
  [ORCHESTRATION_V2_WS_METHODS.dispatchCommand]: AuthOrchestrationOperateScope,
  [WS_METHODS.reachedProbe]: AuthOrchestrationReadScope,
  [WS_METHODS.reachedWatch]: AuthOrchestrationReadScope,
};
