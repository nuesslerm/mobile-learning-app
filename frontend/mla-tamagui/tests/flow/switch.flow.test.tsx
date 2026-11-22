import { fireEvent, renderScreen, userEvent, waitFor, within } from '@lib/flow-test-harness';
import { Switch } from '@lib/tamagui-design-system';
import { useState } from 'react';
import { describe, expect, it } from 'vitest';

function Scene({ initial, onChange, disabled }: { initial: boolean; onChange: (value: boolean) => void; disabled?: boolean }) {
  const [value, setValue] = useState(initial);
  return (
    <Switch
      testID="TheSwitch"
      accessibilityLabel="Show Symbol Bar"
      value={value}
      disabled={disabled}
      onValueChange={(next) => {
        onChange(next);
        setValue(next);
      }}
    />
  );
}

const control = (view: ReturnType<typeof renderScreen>) => within(view.getByTestId('TheSwitch')).getByRole('switch') as HTMLInputElement;

describe('Switch', () => {
  it('Given a switch that is on, When it is toggled, Then onValueChange gets false once and the switch reads off', async () => {
    // Given a switch that is on
    const seen: boolean[] = [];
    const view = renderScreen(<Scene initial onChange={(value) => seen.push(value)} />);
    expect(control(view).checked).toBe(true);

    // When it is toggled
    fireEvent.click(control(view));

    // Then the callback got the new value once and the switch follows it
    await waitFor(() => expect(control(view).checked).toBe(false));
    expect(seen).toEqual([false]);
  });

  it('Given a switch that is off, When it is toggled twice, Then the callback got true and then false', async () => {
    // Given a switch that is off
    const seen: boolean[] = [];
    const view = renderScreen(<Scene initial={false} onChange={(value) => seen.push(value)} />);

    // When it is toggled twice
    fireEvent.click(control(view));
    await waitFor(() => expect(control(view).checked).toBe(true));
    fireEvent.click(control(view));

    // Then the values arrived in order
    await waitFor(() => expect(control(view).checked).toBe(false));
    expect(seen).toEqual([true, false]);
  });

  it('Given a switch with a label, When a screen reader asks for its name, Then it is the label', () => {
    // Given a labelled switch
    const view = renderScreen(<Scene initial onChange={() => {}} />);

    // Then the control is found by that name
    expect(view.getByLabelText('Show Symbol Bar')).toBe(control(view));
  });

  it('Given a disabled switch, When it is pressed, Then nothing is reported', async () => {
    // Given a disabled switch
    const seen: boolean[] = [];
    const view = renderScreen(<Scene initial disabled onChange={(value) => seen.push(value)} />);

    // When it is pressed the way a person presses it, which a disabled control does not take
    await userEvent.click(control(view));

    // Then no value is reported and it stays on
    expect(seen).toEqual([]);
    expect(control(view).checked).toBe(true);
  });

  it('Given a switch with no testID or no label, When it is written, Then the type refuses it', () => {
    // @ts-expect-error a switch has to be addressable
    const noTestID = <Switch value onValueChange={() => {}} accessibilityLabel="x" />;
    // @ts-expect-error a switch has no text of its own, so it has to be named
    const noLabel = <Switch value onValueChange={() => {}} testID="x" />;

    expect([noTestID, noLabel]).toHaveLength(2);
  });
});
