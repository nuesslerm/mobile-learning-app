import { routerHarness, screen, userEvent, waitFor } from '@lib/flow-test-harness';
import { describe, expect, it } from 'vitest';

import { app } from './app';

describe('Algorithms library journey', () => {
  it('Given the Algorithms screen, When I search for two sum and open Two Sum, Then the problem route opens', async () => {
    const router = routerHarness();
    const session = await app.render('/');
    await session.type('AlgorithmsSearch', 'two sum');
    await waitFor(() => {
      expect(screen.queryByTestId('AlgorithmsProblem-add-two-numbers')).toBeNull();
      expect(screen.getByTestId('AlgorithmsProblem-two-sum-bsts')).toBeTruthy();
    });
    await session.tap('AlgorithmsProblem-two-sum');
    await waitFor(() => expect(router.lastPush()).toBe('/leetcode/two-sum/problem'));
  });

  it('Given Two Sum description, When I open Topics and dismiss it, Then the editor action is available again', async () => {
    const session = await app.render('/leetcode/two-sum/problem');
    await session.tap('ProblemInfoOpen-topics');
    await waitFor(() => expect(screen.getByTestId('ProblemInfoTitle').textContent).toBe('Topics'));
    await userEvent.click(screen.getByLabelText('Dismiss Topics'));
    await waitFor(() => expect(screen.queryByTestId('ProblemInfoTitle')).toBeNull());
    expect(screen.getByTestId('ProblemOpenEditor')).toBeTruthy();
  });

  it('Given the problem submissions tab, When I open an accepted submission, Then the matching submission route opens', async () => {
    const router = routerHarness();
    const session = await app.render('/leetcode/two-sum/problem');
    await session.tap('ProblemTab-submissions');
    await session.tap('ProblemSubmission-1');
    await waitFor(() => expect(router.lastPush()).toBe('/leetcode/two-sum/submission/1'));
  });
});
