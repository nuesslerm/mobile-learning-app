import { Alert } from 'react-native';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { installDeviceLab, type DeviceLab } from '@/lib/device-lab';

type LabScope = typeof globalThis & { __deviceLab?: DeviceLab };

const tap = (testId: string) => {
  const el = document.querySelector<HTMLElement>(`[data-testid="${testId}"]`);
  if (!el) throw new Error(`no ${testId} on the screen`);
  el.click();
};

describe('the lab Alert', () => {
  afterEach(() => {
    delete (globalThis as LabScope).__deviceLab;
    document.body.innerHTML = '';
  });

  it('Given a confirm flow, When alerted, Then a dialog appears and the pressed button runs its handler', () => {
    // Given the lab installed and a destructive confirmation with a cancel button
    const lab = installDeviceLab();
    const onDelete = vi.fn();
    const onCancel = vi.fn();

    // When the app alerts
    Alert.alert('Delete draft?', 'This cannot be undone.', [
      { text: 'Cancel', style: 'cancel', onPress: onCancel },
      { text: 'Delete', style: 'destructive', onPress: onDelete },
    ]);

    // Then the dialog shows its title and message, and nothing has run yet
    expect(document.querySelector('[data-testid="LabAlert-Title"]')?.textContent).toBe('Delete draft?');
    expect(document.querySelector('[data-testid="LabAlert-Message"]')?.textContent).toBe('This cannot be undone.');
    expect(onDelete).not.toHaveBeenCalled();

    // When the person taps Delete
    tap('LabAlert-Button-1');

    // Then the handler ran, the dialog is gone, and the lab recorded who answered
    expect(onDelete).toHaveBeenCalledTimes(1);
    expect(onCancel).not.toHaveBeenCalled();
    expect(document.querySelector('[data-testid="LabAlert"]')).toBeNull();
    expect(lab.alerts).toEqual([{ title: 'Delete draft?', message: 'This cannot be undone.', buttons: ['Cancel', 'Delete'], answered: 1 }]);
  });

  it('Given no buttons, When alerted, Then a single OK closes it', () => {
    // Given the lab and an alert with no buttons, as the system treats it
    const lab = installDeviceLab();

    // When the app alerts and the person taps OK
    Alert.alert('Saved');
    tap('LabAlert-Button-0');

    // Then it is answered by the one OK button
    expect(lab.alerts[0]).toMatchObject({ title: 'Saved', buttons: ['OK'], answered: 0 });
  });

  it('Given two alerts at once, When the first is answered, Then the second shows, as the system queue does', () => {
    // Given two alerts raised back to back
    installDeviceLab();
    Alert.alert('First');
    Alert.alert('Second');

    // Then only the first is on the screen
    expect(document.querySelector('[data-testid="LabAlert-Title"]')?.textContent).toBe('First');

    // When it is answered
    tap('LabAlert-Button-0');

    // Then the second takes its place
    expect(document.querySelector('[data-testid="LabAlert-Title"]')?.textContent).toBe('Second');
  });

  it('Given a cancelable alert, When the backdrop is tapped, Then it is dismissed and onDismiss runs', () => {
    // Given a cancelable alert (Android's tap-outside)
    const lab = installDeviceLab();
    const onDismiss = vi.fn();
    Alert.alert('Leave?', undefined, [{ text: 'Stay' }], { cancelable: true, onDismiss });

    // When the backdrop outside the card is tapped
    tap('LabAlert');

    // Then it closed through onDismiss with no button answering
    expect(onDismiss).toHaveBeenCalledTimes(1);
    expect(lab.alerts[0]?.answered).toBeUndefined();
    expect(document.querySelector('[data-testid="LabAlert"]')).toBeNull();
  });
});
