// Environment-aware app config. The "environment" is an inlined EXPO_PUBLIC_*
// value (there are no EAS build profiles — runtime is Expo Go via eas update),
// matching how the logging envs are configured.
//
// `initialRoute` is consumed by app/+native-intent.ts to cold-start the app on a
// chosen screen (no redirect flash). The review default is the LeetCode lecture
// list so the app never strands reviewers inside Storybook.

import { Logger } from '@lib/logging';
import type { Href } from 'expo-router';
import { devRuntimeConfig } from './devRuntimeConfig';
import { resolveInitialRoute } from './resolveInitialRoute';
export type AppEnv = 'development' | 'staging' | 'production';
export const appEnv: AppEnv =
// eslint-disable-next-line no-restricted-syntax -- local escape; switching to Zod/guards would not improve runtime safety here
process.env.EXPO_PUBLIC_APP_ENV as AppEnv | undefined ?? (__DEV__ ? 'development' : 'production');

// A concrete app route, expressed as a string (the string branch of the
// typed-routes `Href`). This lets `initialRoute` flow straight into
// `router.push(...)` with no cast, while staying assignable to `string` for the
// raw-path consumer (`app/+native-intent.ts`).
export type RoutePath = Href & string;
type EnvConfig = {
  /** Route the app opens on at cold start (e.g. '/leetcode/two-sum'). */
  initialRoute?: RoutePath;
  /**
   * Base URL of the agent backend (the BFF that fronts OpenCode + the
   * attachment-conversion service). The in-app chat (`features/agent`) talks to
   * `${agentBaseUrl}/api/convert`.
   * Defaults to a local backend in dev; override at publish time with
   * EXPO_PUBLIC_AGENT_BASE_URL (e.g. the Tailscale URL of the Oracle box).
   */
  agentBaseUrl?: string;
  /**
   * WebSocket URL of the coding-agent orchestration server (backend
   * remote-chat-agent-contracts), the `ws(s)://host/ws?token=<secret>` endpoint
   * the coding-agent client connects to for orchestration RPC. Defaults to
   * unset; override at publish time with EXPO_PUBLIC_CODING_AGENT_WS_URL.
   */
  codingAgentWsUrl?: string;
  /**
   * Base URL of vm-ops-api, the read-only dispatch-runs list (public GETs).
   * The coding-agent dispatch-runs section calls `${vmOpsApiBaseUrl}/opencode-log/...`
   * to list and view runs. Defaults to unset; override at publish time with
   * EXPO_PUBLIC_VM_OPS_API_BASE_URL.
   */
  vmOpsApiBaseUrl?: string;
  /**
   * Endpoint of the optional agentation-mcp server the host agentation feature
   * posts annotations to. When unset, annotations stay local (in-memory +
   * clipboard). Override at publish time with EXPO_PUBLIC_AGENTATION_ENDPOINT.
   */
  agentationEndpoint?: string;
  /**
   * Base URL of leetcode-backend (backend/leetcode-backend), the session-owning
   * service both platforms call identically — there is no more
   * platform-specific auth path. Defaults to a local backend in dev;
   * override at publish time with EXPO_PUBLIC_LEETCODE_BACKEND_BASE_URL
   * (e.g. the Tailscale URL of the Oracle box running backend/leetcode-backend).
   */
  leetcodeBackendBaseUrl?: string;
  /**
   * Bearer token sent as `Authorization: Bearer <token>` on every
   * leetcode-backend request (see requireBearerToken.ts) -- that service's
   * usage routes stopped accepting unauthenticated requests once a public
   * Tailscale Funnel made them reachable beyond the tailnet. Unset in local
   * dev (the local backend enforces the same check; set both to matching
   * values to exercise it locally). Override at publish time with
   * EXPO_PUBLIC_LEETCODE_BACKEND_TOKEN.
   */
  leetcodeBackendToken?: string;
  /**
   * Base URL of the reporting service (backend/reporting-service), the read-only
   * metrics/headroom/plans/runs API behind the agent dashboards. Defaults to
   * unset (the dashboards then show their honest not-configured empty state);
   * override at publish time with EXPO_PUBLIC_REPORTING_BASE_URL (e.g. the
   * Tailscale URL of the Oracle box running backend/reporting-service).
   */
  reportingBaseUrl?: string;
};
const CONFIG: Record<AppEnv, EnvConfig> = {
  development: {
    agentBaseUrl: 'http://localhost:8787',
    vmOpsApiBaseUrl: 'http://vm-ops-api-main:8080',
    leetcodeBackendBaseUrl: 'http://localhost:8789',
    // Not a secret: the seed a local backend starts with (VM_GATEWAY_AUTH_SEED=local-e2e-token). The override
    // below replaces it; overrides only seemed to vanish because Metro's cache ignored them (metro.config.js).
    leetcodeBackendToken: 'local-e2e-token'
  },
  // Staging/review builds boot at the LeetCode lecture list by default. Use
  // EXPO_PUBLIC_INITIAL_ROUTE for a one-off screen-specific preview.
  staging: {
    initialRoute: '/'
  },
  production: {
    initialRoute: '/'
  }
};

// EXPO_PUBLIC_* overrides the per-env defaults when set, so the target screen
// and agent backend can be changed at publish time without a code change.
export const appConfig: EnvConfig = {
  ...CONFIG[appEnv],
  ...(process.env.EXPO_PUBLIC_AGENT_BASE_URL ? {
    agentBaseUrl: process.env.EXPO_PUBLIC_AGENT_BASE_URL
  } : {}),
  ...(process.env.EXPO_PUBLIC_AGENTATION_ENDPOINT ? {
    agentationEndpoint: process.env.EXPO_PUBLIC_AGENTATION_ENDPOINT
  } : {}),
  ...(process.env.EXPO_PUBLIC_CODING_AGENT_WS_URL ? {
    codingAgentWsUrl: process.env.EXPO_PUBLIC_CODING_AGENT_WS_URL
  } : {}),
  ...(process.env.EXPO_PUBLIC_VM_OPS_API_BASE_URL ? {
    vmOpsApiBaseUrl: process.env.EXPO_PUBLIC_VM_OPS_API_BASE_URL
  } : {}),
  ...(process.env.EXPO_PUBLIC_LEETCODE_BACKEND_BASE_URL ? {
    leetcodeBackendBaseUrl: process.env.EXPO_PUBLIC_LEETCODE_BACKEND_BASE_URL
  } : {}),
  ...(process.env.EXPO_PUBLIC_LEETCODE_BACKEND_TOKEN ? {
    leetcodeBackendToken: process.env.EXPO_PUBLIC_LEETCODE_BACKEND_TOKEN
  } : {}),
  ...(process.env.EXPO_PUBLIC_REPORTING_BASE_URL ? {
    reportingBaseUrl: process.env.EXPO_PUBLIC_REPORTING_BASE_URL
  } : {}),
  // eslint-disable-next-line no-restricted-syntax -- local escape; switching to Zod/guards would not improve runtime safety here
  initialRoute: resolveInitialRoute({
    file: appEnv !== 'production' ? devRuntimeConfig.initial_route : null,
    envVar: process.env.EXPO_PUBLIC_INITIAL_ROUTE,
    perEnvDefault: CONFIG[appEnv].initialRoute,
    isProduction: appEnv === 'production'
  }) as RoutePath | undefined
};
Logger.info('config.resolve', {
  appEnv,
  initialRoute: appConfig.initialRoute ?? null,
  agentBaseUrl: appConfig.agentBaseUrl ?? null,
  agentationEndpoint: appConfig.agentationEndpoint ?? null,
  codingAgentWsUrl: appConfig.codingAgentWsUrl ?? null,
  vmOpsApiBaseUrl: appConfig.vmOpsApiBaseUrl ?? null,
  leetcodeBackendBaseUrl: appConfig.leetcodeBackendBaseUrl ?? null,
  reportingBaseUrl: appConfig.reportingBaseUrl ?? null
});