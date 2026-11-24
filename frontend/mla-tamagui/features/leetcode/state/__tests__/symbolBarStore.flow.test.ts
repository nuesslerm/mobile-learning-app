import { act, renderHook } from '@lib/flow-test-harness';
import { beforeEach, describe, expect, it } from 'vitest';

import { forgetLoggedLines, loggedLines } from '../../../../tests/flow/loggedLines';
import { useSymbolBarStore } from '../editor/symbolBarStore';

beforeEach(() => useSymbolBarStore.getState().reset());

describe('the symbol bar store, as logged', () => {
  it('given each setter called once, when the lines are read at the sink, then each one is there under its own event with its fields', async () => {
    await forgetLoggedLines();
    const { result } = renderHook(() => useSymbolBarStore());

    act(() => {
      result.current.setShown(false);
      result.current.setGroup('g2', 'abcdef');
      result.current.move(1, 2);
      result.current.setRailStyle('classic');
      result.current.reset();
    });

    expect((await loggedLines('leetcode.symbol_bar.set_shown'))[0]?.context).toMatchObject({ shown: false });
    expect((await loggedLines('leetcode.symbol_bar.set_group'))[0]?.context).toMatchObject({ id: 'g2', length: 4 });
    expect((await loggedLines('leetcode.symbol_bar.move'))[0]?.context).toMatchObject({ from: 1, to: 2 });
    expect((await loggedLines('leetcode.symbol_bar.set_rail_style'))[0]?.context).toMatchObject({ railStyle: 'classic' });
    expect(await loggedLines('leetcode.symbol_bar.reset')).toHaveLength(1);
  });

  it('given a move to the same place, when the lines are read, then nothing was logged for it', async () => {
    await forgetLoggedLines();
    const { result } = renderHook(() => useSymbolBarStore());

    act(() => result.current.move(3, 3));

    expect(await loggedLines('leetcode.symbol_bar.move')).toHaveLength(0);
  });
});
