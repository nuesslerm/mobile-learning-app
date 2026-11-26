import type { ChatTextPreference } from '@lib/agent-ui-contracts';
import { createFixtureDispatchRunsGateway } from './fixtureDispatchRuns';
import { createFixtureGateway } from './fixtureGateway';
import type { AppearancePreference, CodingAgentPorts } from '../domain/ports';

const FIXTURE_DASHBOARD_BASE_URL = 'http://vm-ops-api-main:8080';

/** Every coding-agent port as an in-memory double: the fixture gateway, the fixture runs and a token held in memory. */
export function createFixturePorts(): CodingAgentPorts {
  let token: string | null = null;
  let chatText: ChatTextPreference | null = null;
  let appearance: AppearancePreference = 'system';
  let notificationsEnabled = false;
  let lastUpdateCheckAt: number | null = null;
  return {
    imageActionsFor: () => ({}),
    copyMessage: async () => true,
    pairingCamera: {
      get: async () => ({ granted: false, canAskAgain: true }),
      request: async () => ({ granted: true, canAskAgain: true }),
    },
    orchestration: { transport: 'fixture', open: async () => createFixtureGateway() },
    dispatchRuns: {
      gateway: createFixtureDispatchRunsGateway(),
      dashboardUrl: (runId) => `${FIXTURE_DASHBOARD_BASE_URL}/opencode-log/${encodeURIComponent(runId)}`,
    },
    tokenStore: {
      read: async () => token,
      write: async (next) => {
        token = next;
      },
      clear: async () => {
        token = null;
      },
    },
    preferences: {
      readChatText: async () => chatText,
      writeChatText: async (next) => { chatText = next; },
      readAppearance: async () => appearance,
      writeAppearance: async (next) => {
        appearance = next;
      },
      readNotificationsEnabled: async () => notificationsEnabled,
      writeNotificationsEnabled: async (next) => {
        notificationsEnabled = next;
      },
      readLastUpdateCheckAt: async () => lastUpdateCheckAt,
      writeLastUpdateCheckAt: async (at) => {
        lastUpdateCheckAt = at;
      },
    },
    appUpdate: {
      checkNow: async () => ({ status: 'up-to-date' as const }),
      install: async () => undefined,
    },
    appInfo: {
      version: async () => '1.0.0',
      build: async () => '1',
    },
    externalLinks: {
      open: async () => undefined,
    },
  };
}
