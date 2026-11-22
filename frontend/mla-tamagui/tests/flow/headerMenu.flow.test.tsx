import {
  renderScreen,
  screen,
  userEvent,
  waitFor,
} from '@lib/flow-test-harness';
import { HeaderMenu, type HeaderMenuItem } from '@lib/tamagui-design-system';
import type { MenuNode } from '@lib/tamagui-design-system';
import { ChevronRight, Info } from '@lib/tamagui-design-system/icons';
import { useState } from 'react';
import { describe, expect, it, vi } from 'vitest';

function EditorMenu({ onSymbols }: { onSymbols: () => void }) {
  const [wrap, setWrap] = useState(true);
  const [whitespace, setWhitespace] = useState(false);
  const [tabs, setTabs] = useState<2 | 4>(4);
  const items: MenuNode[] = [
    {
      testID: 'Wrap',
      label: 'Line wrap',
      icon: Info,
      checked: wrap,
      keepOpen: true,
      onPress: () => setWrap((on) => !on),
    },
    {
      testID: 'Whitespace',
      label: 'Show Whitespace',
      icon: Info,
      checked: whitespace,
      keepOpen: true,
      onPress: () => setWhitespace((on) => !on),
    },
    {
      kind: 'section',
      testID: 'Indent',
      items: [
        {
          testID: 'Tabs2',
          label: 'Tabs: 2',
          checked: tabs === 2,
          keepOpen: true,
          onPress: () => setTabs(2),
        },
        {
          testID: 'Tabs4',
          label: 'Tabs: 4',
          checked: tabs === 4,
          keepOpen: true,
          onPress: () => setTabs(4),
        },
        {
          testID: 'Symbols',
          label: 'Symbol Bar',
          icon: Info,
          onPress: onSymbols,
        },
      ],
    },
  ];
  return (
    <HeaderMenu
      icon={ChevronRight}
      accessibilityLabel="Editor settings"
      items={items}
      size={34}
      testID="EditorMenu"
    />
  );
}

async function openMenu() {
  await userEvent.click(screen.getByTestId('EditorMenu'));
  return waitFor(() => screen.getByTestId('Wrap'));
}

describe('HeaderMenu rendering a menu tree', () => {
  it('Given a tree with toggles, options and a section, When the menu opens, Then checked rows carry a check and aria-checked, other rows none, and the section holds its rows', async () => {
    // Given the menu
    renderScreen(<EditorMenu onSymbols={() => {}} />);

    // When it opens
    await openMenu();

    // Then the toggles and options say whether they are on, and the plain row says nothing
    expect(screen.getByTestId('Wrap').getAttribute('aria-checked')).toBe(
      'true'
    );
    expect(screen.getByTestId('Whitespace').getAttribute('aria-checked')).toBe(
      'false'
    );
    expect(screen.getByTestId('Tabs4').getAttribute('aria-checked')).toBe(
      'true'
    );
    expect(screen.getByTestId('Tabs2').getAttribute('aria-checked')).toBe(
      'false'
    );
    expect(screen.getByTestId('Symbols').hasAttribute('aria-checked')).toBe(
      false
    );
    expect(screen.getByTestId('WrapCheck').querySelector('svg')).not.toBeNull();
    expect(
      screen.getByTestId('WhitespaceCheck').querySelector('svg')
    ).toBeNull();

    // And the section sets its three rows apart from the toggles above
    const section = screen.getByTestId('Indent');
    expect(
      ['Tabs2', 'Tabs4', 'Symbols'].every((id) =>
        section.contains(screen.getByTestId(id))
      )
    ).toBe(true);
    expect(section.contains(screen.getByTestId('Wrap'))).toBe(false);
  });

  it('Given an open menu, When a keep-open toggle is pressed, Then its handler ran and the menu stays open with the check flipped', async () => {
    // Given the open menu with Line wrap on
    renderScreen(<EditorMenu onSymbols={() => {}} />);
    await openMenu();

    // When Line wrap is pressed
    await userEvent.click(screen.getByTestId('Wrap'));

    // Then the check went and every row is still there
    await waitFor(() =>
      expect(screen.getByTestId('Wrap').getAttribute('aria-checked')).toBe(
        'false'
      )
    );
    expect(screen.getByTestId('Whitespace')).not.toBeNull();

    // And pressing it again brings the check back
    await userEvent.click(screen.getByTestId('Wrap'));
    await waitFor(() =>
      expect(screen.getByTestId('Wrap').getAttribute('aria-checked')).toBe(
        'true'
      )
    );
  });

  it('Given an open menu, When a plain action is pressed, Then the menu closes and the handler ran once', async () => {
    // Given the open menu
    const onSymbols = vi.fn();
    renderScreen(<EditorMenu onSymbols={onSymbols} />);
    await openMenu();

    // When Symbol Bar is pressed
    await userEvent.click(screen.getByTestId('Symbols'));

    // Then the handler ran once and the menu is gone
    expect(onSymbols).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(screen.queryByTestId('Symbols')).toBeNull());
  });
});

describe('HeaderMenu given only action rows, as every caller passes today', () => {
  it('Given HeaderMenuItem rows with no kind, When the menu opens, Then each row is a labelled button with its id and no check, and a press closes the menu and acts once', async () => {
    // Given two of today's rows
    const onLogout = vi.fn();
    const items: HeaderMenuItem[] = [
      {
        icon: Info,
        label: 'Version history',
        detail: '3 saved',
        onPress: () => {},
        testID: 'MenuHistory',
      },
      { icon: Info, label: 'Log out', onPress: onLogout, testID: 'MenuLogout' },
    ];
    renderScreen(
      <HeaderMenu
        icon={ChevronRight}
        accessibilityLabel="More"
        items={items}
        size={34}
        testID="MoreMenu"
      />
    );

    // When the menu opens
    await userEvent.click(screen.getByTestId('MoreMenu'));
    const history = await waitFor(() => screen.getByTestId('MenuHistory'));

    // Then the rows are buttons named by their label, with no check state
    expect(history.getAttribute('role')).toBe('button');
    expect(history.getAttribute('aria-label')).toBe('Version history');
    expect(history.hasAttribute('aria-checked')).toBe(false);
    expect(screen.queryByTestId('MenuHistoryCheck')).toBeNull();
    expect(screen.getByText('3 saved')).not.toBeNull();

    // When a row is pressed
    await userEvent.click(screen.getByTestId('MenuLogout'));

    // Then the handler ran once and the menu is gone
    expect(onLogout).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(screen.queryByTestId('MenuHistory')).toBeNull());
  });
});
