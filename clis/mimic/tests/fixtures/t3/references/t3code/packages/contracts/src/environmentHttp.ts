const HOOK_PATH = "/api/hooks/:hookId";

class AuthApi extends HttpApiGroup.make("auth")
  .add(HttpApiEndpoint.get("reachedSession", "/api/auth/session", { success: Session }))
  .add(HttpApiEndpoint.post("unreachedToken", "/oauth/token", { success: Token })) {}

class OrchestrationApi extends HttpApiGroup.make("orchestration")
  .add(HttpApiEndpoint.get("reachedHistory", "/api/orchestration/threads/:threadId/history", { success: Page })) {}

class HooksApi extends HttpApiGroup.make("webhooks")
  .add(HttpApiEndpoint.post("unreachedHook", HOOK_PATH, { success: Accepted })) {}
