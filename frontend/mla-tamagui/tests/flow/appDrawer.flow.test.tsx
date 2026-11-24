import { routerHarness, screen, userEvent, waitFor } from '@lib/flow-test-harness';
import { Keyboard } from 'react-native';
import { describe, expect, it, vi } from 'vitest';

import { app } from './app';

const closeDrawer = () => screen.queryByRole('button', { name: 'Close drawer' });

describe('App drawer', () => {
  it('given any drawer screen, when it renders, then the drawer is mounted but closed', async () => {
    await app.render('/tamil');
    await waitFor(() => {
      expect(screen.getByTestId('TamilLibrary')).toBeTruthy();
    });

    expect(screen.getByTestId('AppDrawer')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Tamil' })).toBeNull();
    expect(closeDrawer()).toBeNull();
    expect(screen.getByTestId('TamilLibrary').closest('[aria-hidden="true"]')).toBeNull();
  });

  it('given the course screen, when its menu button is tapped, then the drawer reveals the active destination', async () => {
    const session = await app.render('/tamil/alphabet');
    await waitFor(() => {
      expect(screen.getByTestId('TamilCourseMenuButton')).toBeTruthy();
    });

    await session.tap('TamilCourseMenuButton');

    await waitFor(() => {
      expect(closeDrawer()).toBeTruthy();
    });
    expect(screen.getByTestId('TamilCourseScreen').closest('[aria-hidden="true"]')).not.toBeNull();
    expect(screen.getByTestId('DrawerItemTamilLabel').textContent).toBe('Tamil');
    expect(screen.getByRole('button', { name: 'Tamil', current: 'page' })).toBeTruthy();
  });

  it('given the open drawer, when a destination is tapped, then the drawer closes and the app pushes that route', async () => {
    const session = await app.render('/tamil/alphabet');
    await waitFor(() => {
      expect(screen.getByTestId('TamilCourseMenuButton')).toBeTruthy();
    });
    await session.tap('TamilCourseMenuButton');
    await waitFor(() => {
      expect(closeDrawer()).toBeTruthy();
    });

    await session.tap('DrawerItemSettings');

    await waitFor(() => {
      expect(routerHarness().lastPush()).toBe('/settings');
      expect(closeDrawer()).toBeNull();
    });
  });

  it('given the menu button, when the drawer opens and the exposed page is tapped, then the keyboard is dismissed and the current page stays mounted', async () => {
    const session = await app.render('/', { prerender: false });
    await screen.findByTestId('LibraryHomeMenuButton');
    // oxlint-disable-next-line no-restricted-properties -- observe the React Native keyboard boundary
    const dismiss = vi.spyOn(Keyboard, 'dismiss');
    try {
      await session.tap('LibraryHomeMenuButton');
      await waitFor(() => expect(dismiss).toHaveBeenCalled());
      await userEvent.setup().click(await screen.findByRole('button', { name: 'Close drawer' }));
      await waitFor(() => expect(closeDrawer()).toBeNull());
      expect(screen.getByTestId('LibraryHome').closest('[aria-hidden="true"]')).toBeNull();
      expect(routerHarness().lastPush()).toBeUndefined();
    } finally {
      dismiss.mockRestore();
    }
  });

  it('given the open drawer, when the account is tapped, then Settings opens over the drawer', async () => {
    const session = await app.render('/');
    await screen.findByTestId('LibraryHomeMenuButton');
    await session.tap('LibraryHomeMenuButton');
    await screen.findByRole('button', { name: 'Open account settings' });
    await session.tap('AppDrawerAccount');
    expect(routerHarness().lastPush()).toBe('/settings');
    expect(closeDrawer()).toBeTruthy();
    expect(screen.getByRole('button', { name: 'LeetCode Library', current: 'page' })).toBeTruthy();
  });

  it('given the open drawer, when the Coding agent shortcut is tapped, then the drawer closes and the app opens Coding agent', async () => {
    const session = await app.render('/');
    await session.tap('LibraryHomeMenuButton');
    await screen.findByRole('button', { name: 'Open Coding agent' });
    await session.tap('AppDrawerCodingAgent');
    await waitFor(() => expect(closeDrawer()).toBeNull());
    expect(routerHarness().lastPush()).toBe('/coding-agent');
  });
});
