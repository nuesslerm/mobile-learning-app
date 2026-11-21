import { createFixtureDispatchRunsGateway } from './fixtureDispatchRuns';
import { createFixtureGateway } from './fixtureGateway';
import type { CodingAgentPorts } from '../domain/ports';

const FIXTURE_DASHBOARD_BASE_URL = 'http://vm-ops-api-main:8080';

/** Every coding-agent port as an in-memory double: the fixture gateway, the fixture runs and a token held in memory. */
export function createFixturePorts(): CodingAgentPorts {
  let token: string | null = null;
  return {
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
  };
}
