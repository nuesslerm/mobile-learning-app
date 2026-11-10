import { screen } from '@lib/flow-test-harness';
import { describe, expect, it } from 'vitest';

import { app } from './app';

describe('app flow (whole app/ glob)', () => {
  it('given the settings index route, when the app renders /settings, then the settings index screen renders', async () => {
    await app.render('/settings');

    expect(screen.getByTestId('SettingsIndexScreen')).toBeTruthy();
    expect(screen.getByText('Profile')).toBeTruthy();
  });

  it('given a path with no matching route, when the app renders /nope, then the pinned error is thrown', async () => {
    await expect(app.render('/nope')).rejects.toThrow(
      'createFlowApp: no route for /nope'
    );
  });
});