import { routerHarness, screen, waitFor } from '@lib/flow-test-harness';
import { describe, expect, it } from 'vitest';

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
});
