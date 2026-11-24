import {
  routerHarness,
  screen,
  setStackHeadersRendered,
  userEvent,
  waitFor,
} from '@lib/flow-test-harness';
import { useNativeHeaderStore } from '@lib/tamagui-design-system';
import { Platform } from 'react-native';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { createHttpReportingGateway } from '../../data/reportingGateway';

import { app } from '../../../../tests/flow/app';
import { forgetLoggedLines, loggedLines } from '../../../../tests/flow/loggedLines';

const realOS = Platform.OS;
const gateway = createHttpReportingGateway(undefined);

const dashboards = [
  ['/dashboards', 'DashboardsIndexScreenHeader', 'dashboards.index.back'],
  ['/dashboards/agents', 'AgentsDashboardScreenHeader', 'dashboards.agents.back'],
  ['/dashboards/infra', 'InfraDashboardScreenHeader', 'dashboards.infra.back'],
] as const;

function render(route: string) {
  return app.render(route, {
    overrides: { dashboards: { reportingGateway: gateway } },
  });
}

beforeEach(async () => {
  useNativeHeaderStore.setState({ enabled: true });
  await forgetLoggedLines();
});

afterEach(() => {
  (Platform as { OS: string }).OS = realOS;
});

describe('Dashboards on the native header', () => {
  it.each(dashboards)(
    'Given %s on the native bar, When the back item is pressed, Then the screen went back and the back line was logged',
    async (route, headerId, line) => {
      // Given the native bar
      (Platform as { OS: string }).OS = 'ios';
      setStackHeadersRendered(true);
      await render(route);
      await waitFor(() => screen.getByTestId('NativeHeaderItem-Go back'));
      expect(screen.queryByTestId(headerId)).toBeNull();

      // When the back item is pressed
      await userEvent.click(screen.getByTestId('NativeHeaderItem-Go back'));

      // Then backTo ran and the line was logged
      expect(routerHarness().calls()).toContainEqual(['back']);
      expect((await loggedLines(line)).length).toBe(1);
    }
  );
});
