import {
  routerHarness,
  screen,
  userEvent,
  waitFor,
} from '@lib/flow-test-harness';
import { describe, expect, it } from 'vitest';

import { app } from './app';

// The home route imports the leetcode feature barrel, which statically pulls in
// ProblemEditorScreen and its native modules that cannot load under jsdom; they
// are never exercised here. The harness has no stubs for them, so they stay
// mocked.



// CollapsingHeader renders Animated.View via the reanimated default import,
// which the harness's reanimated stub cannot satisfy under babel interop; the
// shim keeps the list assertions on the real components and expanded/compact

describe('ProblemListScreen flow (blind75 catalog)', () => {
  it('given the blind75 catalog, when the list renders, then the catalog renders rows with problem titles', async () => {
    await app.render('/');

    await waitFor(() => {
      expect(screen.getByTestId('LeetcodeListScreen')).toBeTruthy();
      expect(screen.getByTestId('LeetcodeListHeroTitle')).toBeTruthy();
    });

    const listItems = screen.getAllByTestId('LeetcodeListItem');
    expect(listItems.length).toBeGreaterThan(0);

    const firstItemTitle = screen.getAllByTestId('LeetcodeListItemTitle')[0];
    expect(firstItemTitle.textContent).toBeTruthy();
    expect(firstItemTitle.textContent).toMatch(/\d+\./);
  });

  it('given a problem row from the blind75 catalog, when tapped, then the app pushes the problem route with the problem id', async () => {
    const router = routerHarness();
    await app.render('/');

    await waitFor(() => {
      const items = screen.getAllByTestId('LeetcodeListItem');
      expect(items.length).toBeGreaterThan(0);
    });

    const firstRow = screen.getAllByTestId('LeetcodeListItem')[0];
    await userEvent.click(firstRow);

    await waitFor(() => {
      const lastPush = router.lastPush();
      expect(lastPush).toMatch(/^\/leetcode\/[a-zA-Z0-9_-]+$/);
    });
  });
});