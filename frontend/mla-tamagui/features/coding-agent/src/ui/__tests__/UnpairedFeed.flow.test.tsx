import { SkinHarness } from '../../testing/SkinHarness';
import { createFlowApp, screen, userEvent } from '@lib/flow-test-harness';
import { describe, expect, it, vi } from 'vitest';
import {
  CodingAgentContainer,
  CODING_AGENT_OVERRIDES,
} from '../../di/CodingAgentContainer';
import { createFixturePorts } from '../../testing/fixturePorts';
import { UnpairedFeed } from '../UnpairedFeed';

describe('Unpaired Orca feed', () => {
  it('Given an unpaired device, when the user taps Pair desktop, then the feed requests pairing once', async () => {
    const requestPairing = vi.fn();
    const app = createFlowApp({
      basePath: '/coding-agent',
  wrapper: SkinHarness,
      routesRoot: '../routes/',
      routes: {
        '../routes/index.tsx': {
          default: () => (
            <CodingAgentContainer>
              <UnpairedFeed onPairDesktop={requestPairing} />
            </CodingAgentContainer>
          ),
        },
      },
    });
    await app.render('/coding-agent', {
      overrides: { [CODING_AGENT_OVERRIDES.name]: createFixturePorts() },
    });
    expect(
      screen.getByRole('heading', { name: 'Connect your desktop' })
    ).toBeTruthy();
    expect(screen.getByText("You're connected")).toBeTruthy();
    await userEvent.click(screen.getByRole('button', { name: 'Pair desktop' }));
    expect(requestPairing).toHaveBeenCalledTimes(1);
  });
});
