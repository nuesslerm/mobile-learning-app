import {
  renderScreen,
  screen,
  screenshot,
  userEvent,
  waitFor,
  within,
} from '@lib/flow-test-harness';
import {
  ScreenHeader,
  type ScreenHeaderProps,
} from '@lib/tamagui-design-system';
import { ChevronRight, Info } from '@lib/tamagui-design-system/icons';
import { describe, expect, it } from 'vitest';

const noop = () => {};

function menu(
  prefix: string,
  label: string
): NonNullable<ScreenHeaderProps['menu']> {
  return {
    icon: ChevronRight,
    accessibilityLabel: `${label} menu`,
    testID: `${prefix}Menu`,
    items: [
      {
        testID: `${prefix}Row`,
        label: `${label} row`,
        icon: Info,
        onPress: noop,
      },
    ],
  };
}

const info = {
  icon: Info,
  accessibilityLabel: 'Info',
  onPress: noop,
  testID: 'BarInfo',
};

function bar(overrides: Partial<ScreenHeaderProps> = {}): ScreenHeaderProps {
  return {
    testID: 'Bar',
    title: 'Two Sum',
    subtitle: 'Array & Hashing · Easy',
    onBack: noop,
    ...overrides,
  };
}

const flankWidth = (testID: string): string =>
  window.getComputedStyle(
    screen.getByTestId(testID).parentElement as HTMLElement
  ).width;

describe('ScreenHeader pills and busy status', () => {
  it('Given actions and a menu only, When rendered, Then one pill holds them under the id it always had and no group or spinner appears', () => {
    // Given today's props
    renderScreen(
      <ScreenHeader {...bar({ actions: [info], menu: menu('Bar', 'More') })} />
    );

    // Then the actions pill is the one the old header drew, with both controls in it
    const pill = screen.getByTestId('BarActions');
    expect(within(pill).getByTestId('BarInfo')).not.toBeNull();
    expect(within(pill).getByTestId('BarMenu')).not.toBeNull();
    expect(screen.queryByTestId('BarGroup0')).toBeNull();
    expect(screen.queryByRole('progressbar')).toBeNull();
  });

  it('Given two groups, When rendered, Then each group is a pill of its own with its members and the right flank grows by the gap between them', () => {
    // Given an Info action beside a menu, and a second menu
    renderScreen(
      <ScreenHeader
        {...bar({
          groups: [
            [menu('Settings', 'Settings')],
            [info, menu('More', 'More')],
          ],
        })}
      />
    );

    // Then two pills hold their own members and the single-pill id is not used
    const first = screen.getByTestId('BarGroup0');
    const second = screen.getByTestId('BarGroup1');
    expect(within(first).getByTestId('SettingsMenu')).not.toBeNull();
    expect(within(second).getByTestId('BarInfo')).not.toBeNull();
    expect(within(second).getByTestId('MoreMenu')).not.toBeNull();
    expect(screen.queryByTestId('BarActions')).toBeNull();

    // And the flank is both pills (46 and 84 wide) and the 8 between them
    expect(flankWidth('BarGroup0')).toBe('138px');
  });

  it('Given a busy status, When it turns on and off, Then a labelled spinner sits before the first pill only while busy', async () => {
    // Given a bar with one pill, not busy
    const view = renderScreen(<ScreenHeader {...bar({ groups: [[info]] })} />);
    expect(screen.queryByRole('progressbar')).toBeNull();

    // When it turns busy
    view.rerender(<ScreenHeader {...bar({ groups: [[info]], busy: true })} />);

    // Then a progress bar with its label comes before the pill in the right flank
    const spinner = screen.getByRole('progressbar');
    expect(spinner.getAttribute('aria-label')).toBe('Working');
    const pill = screen.getByTestId('BarGroup0');
    expect(
      spinner.compareDocumentPosition(pill) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
    await screenshot('while-busy');

    // And it is gone again when the run ends
    view.rerender(<ScreenHeader {...bar({ groups: [[info]], busy: false })} />);
    expect(screen.queryByRole('progressbar')).toBeNull();
  });

  it('Given a very long title, When rendered, Then it stays on one line and both flanks keep the width the controls need', () => {
    // Given a short title and the same bar with a very long one
    const view = renderScreen(<ScreenHeader {...bar({ groups: [[info]] })} />);
    const shortLeft = flankWidth('BarBack');
    const shortRight = flankWidth('BarGroup0');

    // When the title is long
    view.rerender(
      <ScreenHeader
        {...bar({
          title:
            'Longest Substring Without Repeating Characters, With a Very Long Name',
          groups: [[info]],
        })}
      />
    );

    // Then the title is one truncated line and the flanks are as wide as before
    const title = screen.getByTestId('BarTitle');
    expect(window.getComputedStyle(title).whiteSpace).toBe('nowrap');
    expect(flankWidth('BarBack')).toBe(shortLeft);
    expect(flankWidth('BarGroup0')).toBe(shortRight);
  });

  it('Given two menus in two groups, When each is opened, Then each shows its own rows and the other stays closed', async () => {
    // Given two groups with a menu each
    renderScreen(
      <ScreenHeader
        {...bar({
          groups: [[menu('Settings', 'Settings')], [menu('More', 'More')]],
        })}
      />
    );

    // When the first menu is opened
    await userEvent.click(screen.getByTestId('SettingsMenu'));

    // Then its row shows and the other menu's does not
    await waitFor(() => screen.getByTestId('SettingsRow'));
    expect(screen.queryByTestId('MoreRow')).toBeNull();

    // When it is closed by a tap outside and the second is opened
    await userEvent.click(document.body);
    await waitFor(() => expect(screen.queryByTestId('SettingsRow')).toBeNull());
    await userEvent.click(screen.getByTestId('MoreMenu'));

    // Then the second menu's row shows
    await waitFor(() => screen.getByTestId('MoreRow'));
    expect(screen.queryByTestId('SettingsRow')).toBeNull();
  });
});
