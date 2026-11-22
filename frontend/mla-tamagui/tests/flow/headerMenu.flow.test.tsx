import {
  renderScreen,
  screen,
  userEvent,
  waitFor,
  within,
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

const FONT_SIZES = [12, 14, 16, 18] as const;

function SettingsMenu({ onFormat }: { onFormat: () => void }) {
  const [size, setSize] = useState<number>(16);
  const [wrap, setWrap] = useState(true);
  const items: MenuNode[] = [
    {
      kind: 'submenu',
      testID: 'FontSize',
      label: `Font size: ${size}px`,
      icon: Info,
      items: FONT_SIZES.map((candidate) => ({
        testID: `Font${candidate}`,
        label: `${candidate}px`,
        checked: size === candidate,
        keepOpen: true,
        onPress: () => setSize(candidate),
      })),
    },
    {
      testID: 'Wrap',
      label: 'Line wrap',
      icon: Info,
      checked: wrap,
      keepOpen: true,
      onPress: () => setWrap((on) => !on),
    },
    {
      kind: 'confirm',
      testID: 'Format',
      label: 'Format Code',
      icon: Info,
      title: 'Format current code?',
      message: 'Your code will be reformatted.',
      confirmLabel: 'Format',
      onConfirm: onFormat,
    },
  ];
  return (
    <HeaderMenu
      icon={ChevronRight}
      accessibilityLabel="Editor settings"
      items={items}
      size={34}
      testID="SettingsMenu"
    />
  );
}

async function openSettings() {
  await userEvent.click(screen.getByTestId('SettingsMenu'));
  return waitFor(() => screen.getByTestId('FontSize'));
}

describe('HeaderMenu drilling into a submenu and confirming', () => {
  it('Given the root list, When Font size is pressed, Then its rows replace the list in place under a header row, with the current size checked', async () => {
    // Given the open menu
    renderScreen(<SettingsMenu onFormat={() => {}} />);
    await openSettings();

    // When Font size is pressed
    await userEvent.click(screen.getByTestId('FontSize'));

    // Then the sizes are shown under a header row and the root rows are gone
    const header = await waitFor(() => screen.getByTestId('FontSizeHeader'));
    expect(header.getAttribute('aria-label')).toBe('Font size: 16px');
    expect(
      FONT_SIZES.every(
        (candidate) => screen.queryByTestId(`Font${candidate}`) !== null
      )
    ).toBe(true);
    expect(screen.getByTestId('Font16').getAttribute('aria-checked')).toBe(
      'true'
    );
    expect(screen.getByTestId('Font14').getAttribute('aria-checked')).toBe(
      'false'
    );
    expect(screen.queryByTestId('Wrap')).toBeNull();
  });

  it('Given a submenu is open, When its header row is pressed, Then the root list shows again', async () => {
    // Given Font size open
    renderScreen(<SettingsMenu onFormat={() => {}} />);
    await openSettings();
    await userEvent.click(screen.getByTestId('FontSize'));
    await waitFor(() => screen.getByTestId('FontSizeHeader'));

    // When the header row is pressed
    await userEvent.click(screen.getByTestId('FontSizeHeader'));

    // Then the root rows are back and the sizes are gone
    await waitFor(() => screen.getByTestId('Wrap'));
    expect(screen.queryByTestId('Font14')).toBeNull();
  });

  it('Given a submenu is open, When a size is picked, Then the menu stays open on the root with the new value in the Font size row', async () => {
    // Given Font size open
    renderScreen(<SettingsMenu onFormat={() => {}} />);
    await openSettings();
    await userEvent.click(screen.getByTestId('FontSize'));
    await waitFor(() => screen.getByTestId('Font18'));

    // When 18px is picked
    await userEvent.click(screen.getByTestId('Font18'));

    // Then the root shows the new size and the submenu rows are gone
    await waitFor(() =>
      expect(screen.getByTestId('FontSize').getAttribute('aria-label')).toBe(
        'Font size: 18px'
      )
    );
    expect(screen.queryByTestId('Font18')).toBeNull();
  });

  it('Given the root list, When Format Code is pressed, Then a card asks Format current code? with its message and one destructive button', async () => {
    // Given the open menu
    renderScreen(<SettingsMenu onFormat={() => {}} />);
    await openSettings();

    // When Format Code is pressed
    await userEvent.click(screen.getByTestId('Format'));

    // Then the card replaces the list with the title, the message and one button
    const card = await waitFor(() => screen.getByTestId('FormatCard'));
    expect(within(card).getByText('Format current code?')).not.toBeNull();
    expect(
      within(card).getByText('Your code will be reformatted.')
    ).not.toBeNull();
    expect(within(card).getAllByRole('button')).toHaveLength(1);
    expect(screen.getByTestId('FormatConfirm').getAttribute('aria-label')).toBe(
      'Format'
    );
    expect(screen.queryByTestId('Wrap')).toBeNull();
  });

  it('Given the confirmation card, When Format is pressed, Then the handler ran once and the menu closed', async () => {
    // Given the card
    const onFormat = vi.fn();
    renderScreen(<SettingsMenu onFormat={onFormat} />);
    await openSettings();
    await userEvent.click(screen.getByTestId('Format'));
    await waitFor(() => screen.getByTestId('FormatCard'));

    // When Format is pressed
    await userEvent.click(screen.getByTestId('FormatConfirm'));

    // Then the handler ran once and nothing of the menu is left
    expect(onFormat).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(screen.queryByTestId('FormatCard')).toBeNull());
  });

  it('Given the confirmation card, When the person taps outside it, Then the menu closes without formatting and opens on the root next time', async () => {
    // Given the card
    const onFormat = vi.fn();
    renderScreen(<SettingsMenu onFormat={onFormat} />);
    await openSettings();
    await userEvent.click(screen.getByTestId('Format'));
    await waitFor(() => screen.getByTestId('FormatCard'));

    // When the person taps outside
    await userEvent.click(document.body);

    // Then the card is gone and nothing was formatted
    await waitFor(() => expect(screen.queryByTestId('FormatCard')).toBeNull());
    expect(onFormat).not.toHaveBeenCalled();

    // And opening the menu again shows the root list
    await userEvent.click(screen.getByTestId('SettingsMenu'));
    await waitFor(() => screen.getByTestId('FontSize'));
    expect(screen.queryByTestId('FormatCard')).toBeNull();
  });
});
