// features/coding-agent/src/data/codingAgentConfig.ts
//
// Resolves the runtime configuration the orchestration transport needs:
// the WebSocket URL (`wsUrl`) and the bearer `token` (if one was issued by
// the local backend and persisted to AsyncStorage). Kept here in `data/`
// because every consumer — the future integration/state layers and the
// e2e harness — needs the same view of "where do I connect, with what
// credential?", and the resolution is async (AsyncStorage), so it sits
// behind a function rather than a module-level constant.
//
// `wsUrl` precedence: an explicit `appConfig.codingAgentWsUrl` (set by
// `EXPO_PUBLIC_CODING_AGENT_WS_URL`) wins; otherwise the agent's HTTP base
// URL is promoted to a WS URL via `toWsUrl` (http→ws, https→wss, path
// pinned to `/ws`). The token is read from AsyncStorage so the same call
// site works across cold start (no token yet) and a session that has been
// authenticated previously; a missing token is not an error — the server
// may be running with auth disabled.
import { appConfig } from '@lib/app-config';
import { toWsUrl } from '../domain';
import type { TokenStorePort } from '../domain/ports';

export type CodingAgentConfig = {
  wsUrl: string;
  token?: string;
};

export async function resolveCodingAgentConfig(tokenStore: TokenStorePort): Promise<CodingAgentConfig> {
  // `codingAgentWsUrl` lives behind `EXPO_PUBLIC_CODING_AGENT_WS_URL`; the
  // shared `EnvConfig` type doesn't list it yet (it's wired in a follow-up
  // plan task), so we read it through a narrow cast rather than widening
  // the public app-config surface from a data-layer call site.
  const explicitWsUrl = (
    appConfig as { codingAgentWsUrl?: string }
  ).codingAgentWsUrl;
  const agentBaseUrl = appConfig.agentBaseUrl;
  if (explicitWsUrl === undefined && agentBaseUrl === undefined) {
    throw new Error(
      'Cannot resolve coding-agent config: neither appConfig.codingAgentWsUrl nor appConfig.agentBaseUrl is set',
    );
  }
  const wsUrl =
    explicitWsUrl ?? (agentBaseUrl !== undefined ? toWsUrl(agentBaseUrl) : '');
  const stored = await tokenStore.read();
  return stored !== null ? { wsUrl, token: stored } : { wsUrl };
}
