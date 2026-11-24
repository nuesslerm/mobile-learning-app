import type { ComponentType } from 'react';
import {
  act,
  createFlowApp,
  screen,
  userEvent,
  waitFor,
} from '@lib/flow-test-harness';
import { useKeyboardStore } from '@lib/tamagui-design-system';
import { Platform } from 'react-native';
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
    expect(screen.getByRole('button', { name: 'Browse components' })).toBeTruthy();
    await userEvent.click(await screen.findByTestId('ShowcaseItemButton'));

    await screen.findByTestId('ShowcaseItemSheetHeader');
    expect(screen.getByRole('heading', { name: 'Choose a component' })).toBeTruthy();
    for (const group of ['basics', 'inputs', 'surfaces', 'overlays', 'navigation', 'coding-agent']) {
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

  it('given the navigation drawer example, when the drawer is opened and dismissed, then the example stays selected', async () => {
    const session = await app.render('/showcase', { search: { item: 'drawer' } });
    await session.tap('ShowcaseOpenDrawer');

    await userEvent.click(await screen.findByRole('button', { name: 'Close drawer' }));

    await waitFor(() => {
      expect(screen.queryByRole('button', { name: 'Close drawer' })).toBeNull();
      expect(screen.getByTestId('ShowcaseItemTitle').textContent).toBe('Navigation drawer');
    });
  });

  it('given the bottom sheet item, when the compact example is opened, then its sheet shows with a labelled handle', async () => {
    await app.render('/showcase');
    await pick('sheet');
    await userEvent.click(await screen.findByTestId('ShowcaseSheetOpenCompact'));

    expect(await screen.findByTestId('ShowcaseSheetCompactHeader')).toBeTruthy();
    expect(screen.getByTestId('ShowcaseSheetCompactHandle')).toBeTruthy();
  });

  it('given the glass approval picker, when another mode is chosen, then the selection stays checked after reopening', async () => {
    const session = await app.render('/showcase', { search: { item: 'glass-picker' } });
    await session.tap('ShowcaseOpenApprovals');
    expect(await screen.findByRole('radio', { name: 'Approve for me', checked: true })).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Learn more' }).getAttribute('href')).toBe(
      'https://developers.openai.com/codex/agent-approvals-security'
    );

    await session.tap('ShowcaseApproval-full');
    expect(await screen.findByRole('radio', { name: 'Full access', checked: true })).toBeTruthy();

    await session.tap('ShowcaseApprovalSheetHandle');
    expect(screen.queryByRole('radio', { name: 'Full access' })).toBeNull();
    expect(screen.getByTestId('ShowcaseApprovalValue').textContent).toBe('Full access');

    await session.tap('ShowcaseOpenApprovals');
    expect(await screen.findByRole('radio', { name: 'Full access', checked: true })).toBeTruthy();
    await session.tap('ShowcaseApproval-automatic');
    expect(await screen.findByRole('radio', { name: 'Approve for me', checked: true })).toBeTruthy();
  });

  it('given the iOS form sheet, when its field opens the keyboard, then the sheet stays anchored', async () => {
    const platform = Platform as { OS: string };
    const originalOS = platform.OS;
    try {
      const session = await app.render('/showcase', { search: { item: 'sheet' } });
      await screen.findByTestId('ShowcaseSheetOpenForm');
      platform.OS = 'ios';
      await session.tap('ShowcaseSheetOpenForm');
      await session.tap('ShowcaseSheetFormField');

      const sheet = screen.getByTestId('ShowcaseSheetForm').closest('[data-sheet="true"]');
      expect(sheet?.getAttribute('data-keyboard-lift')).toBe('0');
      await act(async () => {
        useKeyboardStore.getState().setKeyboardShown(300);
      });

      expect(sheet?.getAttribute('data-keyboard-lift')).toBe('0');
    } finally {
      await act(async () => {
        useKeyboardStore.getState().setKeyboardHidden();
      });
      platform.OS = originalOS;
    }
  });

  it('given the bottom sheet item, when the scrolling example is opened, then its rows render in the sheet', async () => {
    await app.render('/showcase');
    await pick('sheet');
    await userEvent.click(await screen.findByTestId('ShowcaseSheetOpenScroll'));

    expect(await screen.findByTestId('ShowcaseSheetScrollRow-Row 1')).toBeTruthy();
    expect(screen.getByTestId('ShowcaseNavigationBack')).toBeTruthy();
    await userEvent.click(screen.getByTestId('ShowcaseSheetScrollHeaderClose'));
    await waitFor(() => expect(screen.queryByTestId('ShowcaseSheetScrollHeader')).toBeNull());
    expect(screen.getByTestId('ShowcaseNavigationBack')).toBeTruthy();
  });

  it('given the bottom sheet item, when the Expo scrolling example is opened and closed, then its list appears and is dismissed', async () => {
    await app.render('/showcase', { search: { item: 'sheet' } });
    await userEvent.click(await screen.findByTestId('ShowcaseExpoSheetOpenScroll'));

    expect(await screen.findByText('Row 24')).toBeTruthy();
    await userEvent.click(screen.getByTestId('ShowcaseExpoSheetHeaderClose'));

    await waitFor(() => expect(screen.queryByText('Row 24')).toBeNull());
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
