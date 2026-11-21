import { renderScreen, screen, setSafeAreaDevice } from '@lib/flow-test-harness';
import { describe, expect, it } from 'vitest';

import { TamilWritingPortsProvider } from '../../state/ports';
import { createFakeWritingCheck } from '../../testing';
import { TamilWriteScreen } from '../TamilWriteScreen';

const ports = { writingCheck: createFakeWritingCheck([]) };
const screenUnderTest = (
  <TamilWritingPortsProvider value={ports}>
    <TamilWriteScreen subjectId="uyir-a" />
  </TamilWritingPortsProvider>
);

/** Tamagui puts a pixel padding in the class name (`_pb-34px`), where jsdom has no stylesheet to compute one from. */
function paddingBottomAbove(testId: string): number {
  for (let el = screen.getByTestId(testId).parentElement; el; el = el.parentElement) {
    const match = /(?:^|\s)_pb-(\d+)px(?:\s|$)/.exec(el.getAttribute('class') ?? '');
    if (match) return Number(match[1]);
  }
  return 0;
}

describe('TamilWriteScreen bottom controls', () => {
  it('Given an iPhone 15 Pro, When the write screen renders, Then the controls keep clear of the home indicator', () => {
    // Given the device's bottom inset (34 px)
    setSafeAreaDevice('iphone-15-pro');

    // When the screen renders
    renderScreen(screenUnderTest);

    // Then the container holding "Check my writing" pads its bottom by at least the inset
    expect(paddingBottomAbove('TamilWriteCheck')).toBeGreaterThanOrEqual(34);
  });

  it('Given a phone with no inset, When the write screen renders, Then the usual margin stays', () => {
    // Given a device with no bottom inset
    setSafeAreaDevice('iphone-se');

    // When the screen renders
    renderScreen(screenUnderTest);

    // Then the 12 px margin is kept
    expect(paddingBottomAbove('TamilWriteCheck')).toBe(12);
  });
});
