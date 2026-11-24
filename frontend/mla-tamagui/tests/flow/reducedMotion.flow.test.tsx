import { act, renderScreen, setReducedMotion, waitFor } from '@lib/flow-test-harness';
import { useReducedMotion } from '@lib/tamagui-design-system';
import { Text } from 'react-native';
import { describe, expect, it } from 'vitest';

const Probe = () => <Text testID="Probe">{useReducedMotion() ? 'reduced' : 'full'}</Text>;

describe('useReducedMotion', () => {
  it('Given no preference, When a probe reads the hook, Then it is false', () => {
    const view = renderScreen(<Probe />);

    expect(view.getByTestId('Probe').textContent).toBe('full');
  });

  it('Given the preference is set before the probe mounts, When it reads the hook, Then it is true once the first read settles', async () => {
    setReducedMotion(true);

    const view = renderScreen(<Probe />);

    await waitFor(() => expect(view.getByTestId('Probe').textContent).toBe('reduced'));
  });

  it('Given a mounted probe, When the preference flips on and off, Then the probe follows', async () => {
    const view = renderScreen(<Probe />);

    act(() => setReducedMotion(true));
    await waitFor(() => expect(view.getByTestId('Probe').textContent).toBe('reduced'));

    act(() => setReducedMotion(false));
    await waitFor(() => expect(view.getByTestId('Probe').textContent).toBe('full'));
  });
});
