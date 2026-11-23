import { act, createSession, renderScreen, waitFor } from '@lib/flow-test-harness';
import { SegmentedControl } from '@lib/tamagui-design-system';
import { thumbGeometry } from '@lib/tamagui-design-system/ui/segmented-control';
import { useState } from 'react';
import { describe, expect, it, vi } from 'vitest';

const OPTIONS = [
  { value: 'testcase', label: 'Testcase' },
  { value: 'result', label: 'Result' },
];

function Scene({ variant, onChange }: { variant?: 'thumb'; onChange?: (value: string) => void }) {
  const [value, setValue] = useState('result');
  return (
    <SegmentedControl
      testID="Tabs"
      options={OPTIONS}
      value={value}
      variant={variant}
      onChange={(next) => {
        setValue(next);
        onChange?.(next);
      }}
    />
  );
}

const translateOf = (el: HTMLElement) => /translateX\(([-\d.]+)px\)/.exec(el.style.transform)?.[1];

describe('SegmentedControl thumb variant', () => {
  it('Given variant thumb and value result, When it renders, Then the tablist has two tabs, the second selected, and the thumb is at index 1', async () => {
    const session = createSession(renderScreen(<Scene variant="thumb" />));
    await session.layout('Tabs', { width: 208, height: 40 });

    await waitFor(() => expect(translateOf(session.getByTestId('SegmentedThumb'))).toBe(String(thumbGeometry(1, 2, 208).offset)));
    expect(session.getByRole('tablist')).toBeTruthy();
    const tabs = session.getAllByRole('tab');
    expect(tabs.map((tab) => tab.getAttribute('aria-selected'))).toEqual(['false', 'true']);
  });

  it('Given the other tab, When it is pressed, Then onChange gets its value once and the thumb moves', async () => {
    const onChange = vi.fn();
    const session = createSession(renderScreen(<Scene variant="thumb" onChange={onChange} />));
    await session.layout('Tabs', { width: 208, height: 40 });

    await session.tap('Tabs-testcase');

    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith('testcase');
    await waitFor(() => expect(translateOf(session.getByTestId('SegmentedThumb'))).toBe('0'));
  });

  it('Given no variant, When it renders, Then there is no thumb and no tab role', () => {
    const view = renderScreen(<Scene />);

    expect(view.queryByTestId('SegmentedThumb')).toBeNull();
    expect(view.queryAllByRole('tab')).toHaveLength(0);
    expect(view.container.querySelector('[role="tablist"]')).toBeNull();
  });

  it('Given segments in a 208 wide track, When the geometry is read, Then each is 98 wide and the second is offset by a segment and a gap', () => {
    expect(thumbGeometry(0, 2, 208)).toEqual({ width: 98, offset: 0 });
    expect(thumbGeometry(1, 2, 208)).toEqual({ width: 98, offset: 102 });
    expect(thumbGeometry(1, 2, 0)).toEqual({ width: 0, offset: 0 });
  });

  it('Given an unmeasured track, When it renders, Then the thumb is not drawn wide', async () => {
    const view = renderScreen(<Scene variant="thumb" />);

    await act(async () => undefined);

    expect(view.getByTestId('SegmentedThumb').style.width).toBe('0px');
  });
});
