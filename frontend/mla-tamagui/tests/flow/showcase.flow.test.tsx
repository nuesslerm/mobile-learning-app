import type { ComponentType } from 'react';
import {
  createFlowApp,
  screen,
  userEvent,
  waitFor,
} from '@lib/flow-test-harness';
import { describe, expect, it } from 'vitest';

const app = createFlowApp({
  routes: import.meta.glob<{ default: ComponentType }>('../../app/**/*.tsx'),
  routesRoot: '../../app/',
  includeRootLayout: true,
});

const pick = async (id: string) => {
  await userEvent.click(await screen.findByTestId('ShowcaseItemButton'));
  await userEvent.click(await screen.findByTestId(`ShowcaseItem-${id}`));
};

describe('Design system showcase', () => {
  it('given the showcase route, when it renders, then the first item shows its Tamagui and Expo UI versions', async () => {
    await app.render('/showcase');

    expect((await screen.findByTestId('ShowcaseItemTitle')).textContent).toBe('Text');
    expect(screen.getByTestId('ShowcaseText-TamaguiLabel')).toBeTruthy();
    expect(screen.getByTestId('ShowcaseText-ExpoUi')).toBeTruthy();
  });

  it('given the showcase, when the item button is tapped, then a bottom sheet lists every item under its item type', async () => {
    await app.render('/showcase');
    await userEvent.click(await screen.findByTestId('ShowcaseItemButton'));

    await screen.findByTestId('ShowcaseItemSheetHeader');
    for (const group of ['basics', 'inputs', 'surfaces', 'overlays', 'coding-agent']) {
      expect(screen.getByTestId(`ShowcaseItemGroup-${group}`)).toBeTruthy();
    }
    expect(screen.getByTestId('ShowcaseItem-button')).toBeTruthy();
    expect(screen.getByTestId('ShowcaseItem-sheet')).toBeTruthy();
    expect(screen.getByTestId('ShowcaseItem-diff-view')).toBeTruthy();
  });

  it('given the open picker, when Button is chosen, then the sheet closes and the Button item shows the Expo UI buttons beside the DS ones', async () => {
    await app.render('/showcase');
    await pick('button');

    await waitFor(() => {
      expect(screen.getByTestId('ShowcaseItemTitle').textContent).toBe('Button');
    });
    expect(screen.getByTestId('ButtonDefault')).toBeTruthy();
    expect(screen.getByTestId('ExpoButtonFilled')).toBeTruthy();
    expect(screen.queryByTestId('ShowcaseItemSheetHeader')).toBeNull();
  });

  it('given an item Expo UI has no counterpart for, when Badge is chosen, then the Expo UI section says so', async () => {
    await app.render('/showcase');
    await pick('badge');

    expect(await screen.findByTestId('BadgePrimary')).toBeTruthy();
    expect(screen.getByTestId('ShowcaseBadge-NoExpoUi').textContent).toBe(
      'No Expo UI equivalent.'
    );
  });

  it('given a link with an item id, when the route renders, then that item is shown', async () => {
    await app.render('/showcase', { search: { item: 'diff-view' } });

    expect(await screen.findByTestId('Showcase-CodingAgent-DiffView')).toBeTruthy();
  });

  it('given the bottom sheet item, when the compact example is opened, then its sheet shows with a labelled handle', async () => {
    await app.render('/showcase');
    await pick('sheet');
    await userEvent.click(await screen.findByTestId('ShowcaseSheetOpenCompact'));

    expect(await screen.findByTestId('ShowcaseSheetCompactHeader')).toBeTruthy();
    expect(screen.getByTestId('ShowcaseSheetCompactHandle')).toBeTruthy();
  });

  it('given the bottom sheet item, when the scrolling example is opened, then its rows render in the sheet', async () => {
    await app.render('/showcase');
    await pick('sheet');
    await userEvent.click(await screen.findByTestId('ShowcaseSheetOpenScroll'));

    expect(await screen.findByTestId('ShowcaseSheetScrollRow-Row 1')).toBeTruthy();
  });

  it('given the alert dialog item, when Open AlertDialog is tapped, then the dialog shows', async () => {
    await app.render('/showcase');
    await pick('alert-dialog');
    await userEvent.click(await screen.findByTestId('ShowcaseDestructive'));

    await waitFor(() => {
      expect(screen.getByTestId('ShowcaseAlert')).toBeTruthy();
    });
  });
});
