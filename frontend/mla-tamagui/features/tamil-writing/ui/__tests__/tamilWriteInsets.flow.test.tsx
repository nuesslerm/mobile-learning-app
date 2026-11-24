import { screen, setSafeAreaDevice } from '@lib/flow-test-harness';
import { describe, expect, it } from 'vitest';

import { app } from '../../../../tests/flow/app';

describe('Tamil writing pad bottom controls', () => {
  it.each([['iphone-15-pro', '34px'], ['iphone-se', '12px']] as const)(
    'Given %s, When the write route opens, Then the controls have %s beneath them',
    async (device, padding) => {
      setSafeAreaDevice(device);
      await app.render('/tamil/write/uyir-a');
      const controls = screen.getByTestId('TamilWriteControlsInset');
      expect(controls.style.paddingBottom).toBe(padding);
    }
  );
});
