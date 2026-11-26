import {
  routerHarness,
  screen,
  userEvent,
} from '@lib/flow-test-harness';
import { beforeEach, describe, expect, it } from 'vitest';
import { app } from '../../__tests__/flowApp';

describe('The default Orca home', () => {
  beforeEach(() => {
    routerHarness().reset();
  });

  it('Given the default presentation, when the home renders, then the five Orca tabs show in order with the feed active', async () => {
    await app.render('/coding-agent');
    await screen.findByRole('tab', { name: 'Feed' });

    const tabs = screen.getAllByRole('tab');
    expect(tabs.map((tab) => tab.getAttribute('aria-label'))).toEqual([
      'Feed',
      'Bots',
      'Agents',
      'Usage',
      'Automations',
    ]);
    // The active tab wears the heavier glyph, the way the original marks it.
    expect(tabs[0].querySelector('svg')?.getAttribute('stroke-width')).toBe(
      '2.2'
    );
    expect(tabs[1].querySelector('svg')?.getAttribute('stroke-width')).toBe(
      '1.8'
    );
    expect(screen.getByRole('heading', { name: 'Connect your desktop' })).toBeTruthy();
  });

  it('Given the default home, when the user opens the agents tab, then it shows the agents empty state, its search and its options', async () => {
    await app.render('/coding-agent');
    await screen.findByRole('tab', { name: 'Feed' });

    await userEvent.click(screen.getByRole('tab', { name: 'Agents' }));

    expect(screen.getByRole('heading', { name: 'No agents running' })).toBeTruthy();
    expect(
      screen.getByText('Pair a desktop to see its agents here.')
    ).toBeTruthy();

    await userEvent.click(screen.getByRole('button', { name: 'Search agents' }));
    await userEvent.type(screen.getByTestId('HomeSearchField'), 'codex');
    expect(
      screen.getByRole('heading', { name: 'No agents match “codex”' })
    ).toBeTruthy();

    await userEvent.click(screen.getByRole('button', { name: 'Agent list options' }));
    expect(await screen.findByText('Unread only')).toBeTruthy();
    expect(screen.getByText('Show worker agents')).toBeTruthy();
    expect(screen.getByText('Mark all read')).toBeTruthy();
  });

  it('Given the default home, when the user opens the usage tab, then it shows the usage empty state', async () => {
    await app.render('/coding-agent');
    await screen.findByRole('tab', { name: 'Feed' });

    await userEvent.click(screen.getByRole('tab', { name: 'Usage' }));

    expect(screen.getByRole('heading', { name: 'No usage yet' })).toBeTruthy();
    expect(
      screen.getByText('Pair a desktop to see agent account usage.')
    ).toBeTruthy();
  });

  it('Given the default home, when the user opens the automations tab, then it shows the automations empty state', async () => {
    await app.render('/coding-agent');
    await screen.findByRole('tab', { name: 'Feed' });

    await userEvent.click(screen.getByRole('tab', { name: 'Automations' }));

    expect(screen.getByRole('heading', { name: 'Schedule agent work' })).toBeTruthy();
    expect(
      screen.getByText('Pair a desktop to view and run automations.')
    ).toBeTruthy();
  });

  it('Given an unpaired Feed, when the user opens Bots and returns to the gear, then Bots explains pairing and Settings opens from Feed', async () => {
    await app.render('/coding-agent');
    await userEvent.click(await screen.findByRole('tab', { name: 'Bots' }));
    expect(await screen.findByText('Pair a desktop to see its bots here.')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Settings' })).toBeNull();
    await userEvent.click(screen.getByRole('tab', { name: 'Feed' }));
    await userEvent.click(screen.getByRole('button', { name: 'Settings' }));
    expect(routerHarness().calls()).toContainEqual(['push', '/coding-agent/settings']);
  });

  it('Given the default home, when pairing is opened and dismissed, then the unpaired feed returns', async () => {
    await app.render('/coding-agent');
    await userEvent.click(await screen.findByRole('button', { name: 'Pair desktop' }));
    await userEvent.click(await screen.findByRole('button', { name: 'Back' }));
    expect(await screen.findByRole('heading', { name: 'Connect your desktop' })).toBeTruthy();
  });
});
