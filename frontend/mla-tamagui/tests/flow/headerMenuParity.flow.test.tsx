import {
  act,
  renderScreen,
  screen,
  setStackHeadersRendered,
  userEvent,
  waitFor,
} from '@lib/flow-test-harness';
import {
  ScreenHeader,
  useNativeHeaderStore,
  type MenuNode,
} from '@lib/tamagui-design-system';
import { ChevronRight } from '@lib/tamagui-design-system/icons';
import { useState } from 'react';
import { Platform } from 'react-native';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

const realOS = Platform.OS;
const setPlatform = (os: 'web' | 'ios') => {
  (Platform as { OS: string }).OS = os;
};

beforeEach(() => {
  act(() => {
    useNativeHeaderStore.setState({ enabled: true });
  });
});

afterEach(() => {
  setPlatform(realOS as 'web');
});

function Probe({ calls, glass }: { calls: string[]; glass: boolean }) {
  const [wrap, setWrap] = useState(true);
  const [size, setSize] = useState(14);
  const items: MenuNode[] = [
    {
      testID: 'Wrap',
      label: 'Line wrap',
      checked: wrap,
      keepOpen: true,
      onPress: () => {
        calls.push('wrap');
        setWrap((on) => !on);
      },
    },
    {
      kind: 'submenu',
      testID: 'FontSize',
      label: 'Font size',
      items: [14, 16].map((candidate) => ({
        testID: `Font${candidate}`,
        label: `${candidate}px`,
        checked: size === candidate,
        keepOpen: true,
        onPress: () => {
          calls.push(`size:${candidate}`);
          setSize(candidate);
        },
      })),
    },
    {
      testID: 'Symbols',
      label: 'Symbol Bar',
      onPress: () => calls.push('symbols'),
    },
    {
      kind: 'confirm',
      testID: 'Format',
      label: 'Format Code',
      title: 'Format current code?',
      message: 'Your code will be reformatted.',
      confirmLabel: 'Format',
      onConfirm: () => calls.push('format'),
    },
  ];
  return (
    <ScreenHeader
      testID="Bar"
      title="Editor"
      glass={glass}
      groups={[
        [
          {
            testID: 'BarSettings',
            accessibilityLabel: 'Editor settings',
            icon: ChevronRight,
            sfSymbol: 'slider.horizontal.3',
            items,
          },
        ],
      ]}
    />
  );
}

type Seen = {
  open: boolean;
  view: 'root' | 'submenu' | 'confirm';
  checked: Record<string, boolean>;
};
const CHECKABLE = ['Line wrap', '14px', '16px'];

type Menu = {
  open: () => Promise<void>;
  press: (label: string) => Promise<void>;
  back: () => Promise<void>;
  seen: () => Seen;
};

const readChecked = (find: (label: string) => HTMLElement | null) =>
  Object.fromEntries(
    CHECKABLE.flatMap((label) => {
      const state = find(label)?.getAttribute('aria-checked');
      return state === null || state === undefined
        ? []
        : [[label, state === 'true']];
    })
  );

const fallback: Menu = {
  open: () => userEvent.click(screen.getByTestId('BarSettings')),
  press: (label) => userEvent.click(screen.getByLabelText(label)),
  back: () => userEvent.click(screen.getByTestId('FontSizeHeader')),
  seen: () => ({
    open: screen.queryByTestId('BarSettingsContent') !== null,
    view: screen.queryByTestId('FormatCard')
      ? 'confirm'
      : screen.queryByTestId('FontSizeHeader')
        ? 'submenu'
        : 'root',
    checked: readChecked((label) => screen.queryByLabelText(label)),
  }),
};

const nativeRow = (label: string) =>
  screen.queryByTestId(`NativeHeaderMenuItem-${label}`);
const twin: Menu = {
  open: () =>
    userEvent.click(screen.getByTestId('NativeHeaderItem-Editor settings')),
  press: (label) =>
    userEvent.click(screen.getByTestId(`NativeHeaderMenuItem-${label}`)),
  back: () =>
    userEvent.click(
      document.querySelector(
        '[data-testid^="NativeHeaderMenuHeader-"]'
      ) as HTMLElement
    ),
  seen: () => {
    const header = document.querySelector(
      '[data-testid^="NativeHeaderMenuHeader-"]'
    );
    return {
      open: screen.queryByTestId('NativeHeaderMenu-Editor settings') !== null,
      view: header
        ? header.getAttribute('data-testid')?.endsWith('Format Code')
          ? 'confirm'
          : 'submenu'
        : 'root',
      checked: readChecked(nativeRow),
    };
  },
};

type Step = { do: 'open' } | { do: 'press'; label: string } | { do: 'back' };

const ROOT_OPEN: Seen = {
  open: true,
  view: 'root',
  checked: { 'Line wrap': true },
};

const BEHAVIOURS: { name: string; steps: [Step, Seen][]; calls: string[] }[] = [
  {
    name: 'a toggle keeps the menu open and flips its check',
    steps: [
      [{ do: 'open' }, ROOT_OPEN],
      [
        { do: 'press', label: 'Line wrap' },
        { ...ROOT_OPEN, checked: { 'Line wrap': false } },
      ],
    ],
    calls: ['wrap'],
  },
  {
    name: 'a submenu replaces the list in place and its header row returns',
    steps: [
      [{ do: 'open' }, ROOT_OPEN],
      [
        { do: 'press', label: 'Font size' },
        {
          open: true,
          view: 'submenu',
          checked: { '14px': true, '16px': false },
        },
      ],
      [{ do: 'back' }, ROOT_OPEN],
    ],
    calls: [],
  },
  {
    name: 'an option picked in a submenu runs once and returns to the root',
    steps: [
      [{ do: 'open' }, ROOT_OPEN],
      [
        { do: 'press', label: 'Font size' },
        {
          open: true,
          view: 'submenu',
          checked: { '14px': true, '16px': false },
        },
      ],
      [{ do: 'press', label: '16px' }, ROOT_OPEN],
    ],
    calls: ['size:16'],
  },
  {
    name: 'a confirm row asks first and runs once when its button is pressed',
    steps: [
      [{ do: 'open' }, ROOT_OPEN],
      [
        { do: 'press', label: 'Format Code' },
        { open: true, view: 'confirm', checked: {} },
      ],
      [
        { do: 'press', label: 'Format' },
        { open: false, view: 'root', checked: {} },
      ],
    ],
    calls: ['format'],
  },
  {
    name: 'a plain action runs once and closes the menu',
    steps: [
      [{ do: 'open' }, ROOT_OPEN],
      [
        { do: 'press', label: 'Symbol Bar' },
        { open: false, view: 'root', checked: {} },
      ],
    ],
    calls: ['symbols'],
  },
];

async function run(menu: Menu, steps: [Step, Seen][]) {
  for (const [step, expected] of steps) {
    if (step.do === 'open') await menu.open();
    else if (step.do === 'press') await menu.press(step.label);
    else await menu.back();
    await waitFor(() => expect(menu.seen()).toEqual(expected));
  }
}

describe('One menu tree through the menu panel and the native twin', () => {
  it.each(BEHAVIOURS)(
    'Given the same tree, When $name, Then the panel and the twin end each step alike with the same handler calls',
    async ({ steps, calls: expectedCalls }) => {
      // Given the tree in the menu panel (Android and web)
      const fallbackCalls: string[] = [];
      const first = renderScreen(<Probe calls={fallbackCalls} glass />);

      // When the behaviour runs through it
      await run(fallback, steps);

      // Then it ended as expected, having run the handlers the behaviour names
      expect(fallbackCalls).toEqual(expectedCalls);
      first.unmount();

      // Given the same tree on the native bar (iOS, the setting on)
      setPlatform('ios');
      setStackHeadersRendered(true);
      const twinCalls: string[] = [];
      renderScreen(<Probe calls={twinCalls} glass />);

      // When the same behaviour runs through the twin
      await run(twin, steps);

      // Then it ended in the same states with the same handler calls
      expect(twinCalls).toEqual(expectedCalls);
    }
  );
});
