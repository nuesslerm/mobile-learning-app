import { routerHarness, screen, userEvent, waitFor } from '@lib/flow-test-harness';
import { describe, expect, it } from 'vitest';

import { app } from './app';

describe('Algorithms library journey', () => {
  it('Given the Library, When I choose NeetCode 150, Then it opens the dedicated collection', async () => {
    const router = routerHarness();
    const home = await app.render('/');
    await home.tap('LibraryCategory-NeetCode 150');
    await waitFor(() => expect(JSON.stringify(router.lastPush())).toContain('NeetCode'));
  });

  it('Given a problem description, When I repeatedly tap Play, Then it opens exactly one editor', async () => {
    const detail = await app.render('/leetcode/car-fleet/problem');
    const router = routerHarness();
    for (let i = 0; i < 5; i++) await detail.tap('ProblemOpenEditor');
    expect(router.pushes()).toEqual([{ pathname: '/leetcode/[id]', params: { id: 'car-fleet', lang: 'javascript' } }]);
  });

  it('Given a NeetCode problem beyond Blind 75, When I open its lesson, Then its video and worked approaches are available', async () => {
    const detail = await app.render('/leetcode/car-fleet/problem');
    await detail.tap('ProblemOpenLesson');
    expect(screen.queryByTitle('Car Fleet video explanation')).toBeNull();
    await detail.tap('LessonOpenVideo');
    expect(screen.getByTitle('Car Fleet video explanation').getAttribute('src')).toBe('https://www.youtube.com/embed/Pr6T-3yB9RM?playsinline=1');
    expect(screen.getByTestId('NeetCodeLesson').textContent).toContain('class Solution');
  });

  it('Given the Algorithms screen, When I search for two sum and open Two Sum, Then the problem route opens', async () => {
    const router = routerHarness();
    const session = await app.render('/leetcode/library/collection');
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

  it('Given a real problem, When I open its description, Then its solutions link points to the original problem and no voting controls are offered', async () => {
    await app.render('/leetcode/two-sum/problem');
    expect(screen.getByText('Community solutions on LeetCode ↗').closest('a')?.getAttribute('href')).toBe('https://leetcode.com/problems/two-sum/solutions/');
    expect(screen.queryByLabelText('Like')).toBeNull();
    expect(screen.queryByLabelText('Dislike')).toBeNull();
    expect(screen.queryByLabelText('Discussion')).toBeNull();
  });

  it('Given an unknown problem link, When I open it, Then the app shows an unavailable message without exercise actions', async () => {
    await app.render('/leetcode/unknown-exercise/problem');
    expect(screen.getByText('This problem is unavailable in the local collection.')).toBeTruthy();
    expect(screen.queryByTestId('ProblemOpenEditor')).toBeNull();
    expect(screen.queryByTestId('ProblemCommunitySolutions')).toBeNull();
    expect(screen.queryByTestId('ProblemOpenLesson')).toBeNull();
  });

  it('Given a problem outside NeetCode 150, When I open it, Then I can read the exercise without a placeholder lesson', async () => {
    await app.render('/leetcode/two-sum-less-than-k/problem');
    expect(screen.getByText('1099. Two Sum Less Than K')).toBeTruthy();
    expect(screen.queryByTestId('ProblemOpenLesson')).toBeNull();
    expect(screen.queryByTestId('ProblemTab-lesson')).toBeNull();
  });
});
