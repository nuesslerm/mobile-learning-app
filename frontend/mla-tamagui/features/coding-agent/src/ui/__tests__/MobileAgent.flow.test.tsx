import { routerHarness, screen, userEvent, waitFor } from '@lib/flow-test-harness';
import { describe, expect, it } from 'vitest';
import { app } from '../../__tests__/flowApp';
import { createMobileDouble } from '../../testing/mobilePort';

describe('RCA mobile routes', () => {
  it('given a new device, when it pairs and starts a task, then its RCA thread opens', async () => {
    const mobile = createMobileDouble();
    const session = await app.render('/coding-agent', { overrides: { 'coding-agent': { mobile } } });
    await userEvent.type(await screen.findByTestId('MobilePairEndpoint'), 'https://host.example');
    await userEvent.type(screen.getByTestId('MobilePairCode'), 'issued-code');
    await session.tap('MobilePair');
    await screen.findByTestId('MobileWorkspace-mobile-app');
    await session.tap('MobileWorkspace-mobile-app');
    await screen.findByTestId('MobileTask-42');
    await session.tap('MobileTask-42');
    await waitFor(() => expect(routerHarness().lastPush()).toContain('thread-42'));
  });
  it('given a reopened thread, when a pending approval is answered, then the transcript and child activity remain', async () => {
    const mobile = createMobileDouble({ endpoint: 'https://host.example', token: 'example' });
    await app.render('/coding-agent/thread-42', { overrides: { 'coding-agent': { mobile } } });
    await screen.findByText('Check selection');
    expect(screen.getByText('Task #42')).toBeTruthy();
    await userEvent.click(screen.getByText('/root/reviewer'));
    await screen.findByText('Selection restored');
    await userEvent.click(screen.getByText('Allow once'));
    await waitFor(() => expect(screen.queryByText('Approval requested')).toBeNull());
    expect(screen.getByText('Check selection')).toBeTruthy();
  });
});
