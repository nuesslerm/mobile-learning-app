import { renderScreen, waitFor } from '@lib/flow-test-harness';
import { Sheet } from '@lib/tamagui-design-system/ui/sheet';
import { Keyboard, Platform, TextInput } from 'react-native';
import { afterEach, describe, expect, it, vi } from 'vitest';

/**
 * The design-system Sheet forks on Platform.OS twice (device-lab spec U5): on web it blurs the focused element when it
 * opens and gives the pan handler a vertical activation window; on a phone it does neither. react-native-web reports
 * 'web', so the native branch is run by naming the platform.
 */
const realOS = Platform.OS;

function setPlatform(os: 'web' | 'ios') {
  (Platform as { OS: string }).OS = os;
}

const Scene = ({ open }: { open: boolean }) => (
  <>
    <TextInput testID="Field" />
    <Sheet open={open} onOpenChange={() => {}} testID="TheSheet">
      <></>
    </Sheet>
  </>
);

async function openSheetBesideFocusedInput() {
  // oxlint-disable-next-line no-restricted-properties -- spy on the React Native Keyboard module, not a project module
  const dismiss = vi.spyOn(Keyboard, 'dismiss').mockImplementation(() => {});
  const view = renderScreen(<Scene open={false} />);
  const field = view.getByTestId('Field') as HTMLInputElement;
  field.focus();
  expect(document.activeElement).toBe(field);
  view.rerender(<Scene open />);
  const sheet = await waitFor(() => {
    const el = document.querySelector('[data-sheet="true"]');
    if (!el) throw new Error('the sheet has not mounted');
    return el;
  });
  return { field, sheet, dismiss };
}

describe('Sheet platform branches', () => {
  afterEach(() => {
    setPlatform(realOS as 'web');
    vi.restoreAllMocks();
    document.body.innerHTML = '';
  });

  it('Given the web branch, When the sheet opens over a focused field, Then the field is blurred and the pan handler has its vertical window', async () => {
    // Given the browser render and a focused field
    setPlatform('web');

    // When the sheet opens
    const { field, sheet, dismiss } = await openSheetBesideFocusedInput();

    // Then the keyboard was put away, focus left the field, and the web-only activation window is set
    await waitFor(() => expect(document.activeElement).not.toBe(field));
    expect(dismiss).toHaveBeenCalled();
    expect(sheet.getAttribute('data-active-offset-y')).toBe('[-15,15]');
  });

  it('Given the native branch, When the sheet opens over a focused field, Then only Keyboard.dismiss runs and no web window is set', async () => {
    // Given a phone
    setPlatform('ios');

    // When the sheet opens
    const { field, sheet, dismiss } = await openSheetBesideFocusedInput();

    // Then the OS keyboard is dismissed through Keyboard.dismiss, with no DOM blur and no web-only prop
    await waitFor(() => expect(dismiss).toHaveBeenCalled());
    expect(document.activeElement).toBe(field);
    expect(sheet.getAttribute('data-active-offset-y')).toBeNull();
  });
});
