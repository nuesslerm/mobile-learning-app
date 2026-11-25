import { Logger } from '@lib/logging';
import { expect, it } from 'vitest';

import { forgetLoggedLines, loggedLines } from './loggedLines';

it('given console capture after logger startup, when the app logs a route change and an editor timing, then each reaches the console once', async () => {
  await forgetLoggedLines();

  Logger.info('route.change', { to: '/settings' });
  Logger.info('editor.input_timing', { updates: 3 });

  expect(await loggedLines()).toMatchObject([
    { level: 'info', text: 'route.change', context: { to: '/settings' } },
    { level: 'info', text: 'editor.input_timing', context: { updates: 3 } },
  ]);
});
