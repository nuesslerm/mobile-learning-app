import {
  routerHarness,
  screen,
  userEvent,
  waitFor,
} from '@lib/flow-test-harness';
import { beforeEach, describe, expect, it } from 'vitest';
import { app } from '../../__tests__/flowApp';
import { createMobileDouble } from '../../testing/mobilePort';

const STAGED = { search: { presentation: 'orca' } };
// A fresh double per test: pairing one of them pairs that device only.
const mobile = () => ({ mobile: createMobileDouble() });

describe('The staged Orca home', () => {
  beforeEach(() => {
    routerHarness().reset();
  });

  it('Given the staged presentation, when the home renders, then the five Orca tabs show in order with the feed active', async () => {
    await app.render('/coding-agent', {
      ...STAGED,
      overrides: { 'coding-agent': mobile() },
    });

    const tabs = screen.getAllByRole('tab');
    expect(tabs.map((tab) => tab.getAttribute('aria-label'))).toEqual([
      'Feed',
      'Agents',
      'Usage',
      'Automations',
      'Settings',
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

  it('Given the staged home, when the user opens the agents tab, then it shows the agents empty state, its search and its options', async () => {
    await app.render('/coding-agent', {
      ...STAGED,
      overrides: { 'coding-agent': mobile() },
    });

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

  it('Given the staged home, when the user opens the usage tab, then it shows the usage empty state', async () => {
    await app.render('/coding-agent', {
      ...STAGED,
      overrides: { 'coding-agent': mobile() },
    });

    await userEvent.click(screen.getByRole('tab', { name: 'Usage' }));

    expect(screen.getByRole('heading', { name: 'No usage yet' })).toBeTruthy();
    expect(
      screen.getByText('Pair a desktop to see agent account usage.')
    ).toBeTruthy();
  });

  it('Given the staged home, when the user opens the automations tab, then it shows the automations empty state', async () => {
    await app.render('/coding-agent', {
      ...STAGED,
      overrides: { 'coding-agent': mobile() },
    });

    await userEvent.click(screen.getByRole('tab', { name: 'Automations' }));

    expect(screen.getByRole('heading', { name: 'Schedule agent work' })).toBeTruthy();
    expect(
      screen.getByText('Pair a desktop to view and run automations.')
    ).toBeTruthy();
  });

  it('Given the staged home, when the user opens the settings tab, then it carries the original\'s menu', async () => {
    await app.render('/coding-agent', {
      ...STAGED,
      overrides: { 'coding-agent': mobile() },
    });

    await userEvent.click(screen.getByRole('tab', { name: 'Settings' }));

    expect(screen.getByText('Appearance')).toBeTruthy();
    expect(screen.getByText('Terminal')).toBeTruthy();
    expect(screen.getByText('Chat UI')).toBeTruthy();
    expect(screen.getByText('Browser')).toBeTruthy();
    expect(screen.getByText('Voice')).toBeTruthy();
    expect(screen.getByText('Notifications')).toBeTruthy();
    expect(screen.getByText('Troubleshooting')).toBeTruthy();
    expect(screen.getByText('About')).toBeTruthy();
    expect(screen.getByText('Privacy Policy')).toBeTruthy();
    expect(screen.getByText('Support')).toBeTruthy();
  });

  it('Given the staged home, when the user pairs a desktop, then the pairing runs and the coding agent takes over', async () => {
    await app.render('/coding-agent', {
      ...STAGED,
      overrides: { 'coding-agent': mobile() },
    });

    await userEvent.click(screen.getByRole('button', { name: 'Pair desktop' }));
    expect(screen.getByTestId('MobilePairEndpoint')).toBeTruthy();

    await userEvent.type(
      screen.getByTestId('MobilePairEndpoint'),
      'https://host.example'
    );
    await userEvent.type(screen.getByTestId('MobilePairCode'), 'issued-code');
    await userEvent.click(screen.getByRole('button', { name: 'Pair device' }));

    await waitFor(() =>
      expect(routerHarness().calls()).toContainEqual(['replace', '/coding-agent'])
    );
  });

  it('Given the staged home, when the user leaves the pairing, then the unpaired feed is back', async () => {
    await app.render('/coding-agent', {
      ...STAGED,
      overrides: { 'coding-agent': mobile() },
    });

    await userEvent.click(screen.getByRole('button', { name: 'Pair desktop' }));
    expect(screen.getByTestId('MobilePairEndpoint')).toBeTruthy();

    await userEvent.click(screen.getByRole('button', { name: 'Back to feed' }));
    expect(
      screen.getByRole('heading', { name: 'Connect your desktop' })
    ).toBeTruthy();
  });

  it('Given the coding agent home without the staged presentation, then it keeps the home it had', async () => {
    await app.render('/coding-agent', {
      overrides: { 'coding-agent': mobile() },
    });

    expect(screen.queryByRole('tab')).toBeNull();
    expect(await screen.findByTestId('MobilePairEndpoint')).toBeTruthy();
  });
});
