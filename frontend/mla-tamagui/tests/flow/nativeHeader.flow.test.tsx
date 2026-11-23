import {
  act,
  FlowProviders,
  renderHook,
  renderScreen,
  screen,
  setLiquidGlass,
  setStackHeadersRendered,
  userEvent,
  waitFor,
  within,
} from '@lib/flow-test-harness';
import {
  ScreenHeader,
  useGlassHeaderStore,
  useHeaderHeightStore,
  useScreenHeaderOptions,
  type ScreenHeaderProps,
} from '@lib/tamagui-design-system';
import { ChevronRight, Info } from '@lib/tamagui-design-system/icons';
import { Platform, Text } from 'react-native';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const realOS = Platform.OS;

function setPlatform(os: 'web' | 'ios' | 'android') {
  (Platform as { OS: string }).OS = os;
}

function glassSetting(enabled: boolean) {
  act(() => {
    useGlassHeaderStore.setState({ enabled });
  });
}

function native() {
  setPlatform('ios');
  setLiquidGlass(true);
  setStackHeadersRendered(true);
  glassSetting(true);
}

beforeEach(() => {
  glassSetting(false);
  useHeaderHeightStore.setState({ height: 0 });
});

// jsdom has no viewport, so a window is 0 wide until a test gives it a width, as a phone's is.
function setWindowWidth(width: number | null) {
  if (width === null)
    Reflect.deleteProperty(document.documentElement, 'clientWidth');
  else
    Object.defineProperty(document.documentElement, 'clientWidth', {
      configurable: true,
      value: width,
    });
  act(() => {
    window.dispatchEvent(new Event('resize'));
  });
}

// The width goes first: its resize re-renders the screen, and the golden of the test's last screen is taken after this hook.
afterEach(() => {
  setWindowWidth(null);
  setPlatform(realOS as 'web');
});

type ProbeProps = {
  glass?: boolean;
  title?: string;
  onBack?: () => void;
  onInfo?: () => void;
  onWrap?: () => void;
  wrap?: boolean;
  crowded?: boolean;
};

function bar({
  glass = true,
  title = 'Appearance',
  onBack = () => {},
  onInfo = () => {},
  onWrap = () => {},
  wrap = true,
  crowded = false,
}: ProbeProps): ScreenHeaderProps {
  return {
    testID: 'Bar',
    title,
    subtitle: 'Settings',
    glass,
    onBack,
    groups: [
      [
        {
          icon: ChevronRight,
          sfSymbol: 'slider.horizontal.3',
          accessibilityLabel: 'Editor settings',
          testID: 'BarSettings',
          items: [
            {
              testID: 'BarWrap',
              label: 'Line wrap',
              checked: wrap,
              keepOpen: true,
              onPress: onWrap,
            },
          ],
        },
      ],
      [
        {
          icon: Info,
          sfSymbol: 'info.circle',
          accessibilityLabel: 'Info',
          testID: 'BarInfo',
          onPress: onInfo,
        },
        ...(crowded
          ? ['BarShare', 'BarSave'].map((id) => ({
              icon: Info,
              sfSymbol: 'square.and.arrow.up' as const,
              accessibilityLabel: id,
              testID: id,
              onPress: onInfo,
            }))
          : []),
      ],
    ],
  };
}

const Probe = (props: ProbeProps) => (
  <>
    <ScreenHeader {...bar(props)} />
    <Text testID="Body">Body</Text>
  </>
);

describe('ScreenHeader on the native glass bar', () => {
  it('Given iOS with Liquid Glass, the setting on and a screen that opted in, When it mounts, Then the twin shows the back item and the pieces of glass and the Tamagui header is absent', () => {
    // Given the native environment
    native();

    // When the screen mounts
    renderScreen(<Probe />);

    // Then the bar is native: a round back item, the settings piece alone, Info alone
    expect(screen.getByTestId('NativeHeader')).not.toBeNull();
    expect(
      screen
        .getByTestId('NativeHeaderItem-Go back')
        .getAttribute('data-shares-background')
    ).toBe('false');
    expect(
      screen.getByTestId('NativeHeaderItem-Editor settings')
    ).not.toBeNull();
    expect(screen.getByTestId('NativeHeaderItem-Info')).not.toBeNull();
    expect(
      screen.getByTestId('NativeHeader').getAttribute('data-header-transparent')
    ).toBe('false');

    // And the Tamagui header did not draw, while the body did
    expect(screen.queryByTestId('Bar')).toBeNull();
    expect(screen.queryByTestId('BarBack')).toBeNull();
    expect(screen.getByTestId('Body')).not.toBeNull();
  });

  it.each([
    ['the setting is off', () => glassSetting(false)],
    ['the platform is android', () => setPlatform('android')],
    ['Liquid Glass is not available', () => setLiquidGlass(false)],
  ])(
    'Given the native environment except that %s, When the screen mounts, Then the Tamagui header draws with its ids and the twin shows no bar',
    (_name, spoil) => {
      // Given the native environment spoiled in one way
      native();
      spoil();

      // When the screen mounts
      renderScreen(<Probe />);

      // Then the classic header is there and nothing native
      expect(screen.getByTestId('Bar')).not.toBeNull();
      expect(screen.getByTestId('BarBack')).not.toBeNull();
      expect(screen.getByTestId('BarGroup0')).not.toBeNull();
      expect(screen.queryByTestId('NativeHeader')).toBeNull();
    }
  );

  it('Given the native environment, When the screen did not pass glass, Then the Tamagui header draws', () => {
    // Given the native environment
    native();

    // When a screen mounts without opting in
    renderScreen(<Probe glass={false} />);

    // Then it keeps its Tamagui header
    expect(screen.getByTestId('Bar')).not.toBeNull();
    expect(screen.queryByTestId('NativeHeader')).toBeNull();
  });

  it('Given the native bar, When the screen re-renders with new closures and the same data, Then the bar is not rebuilt, and when the data changes it is', () => {
    // Given the native bar
    native();
    const view = renderScreen(<Probe />);
    const version = () =>
      screen.getByTestId('NativeHeader').getAttribute('data-options-version');
    const first = version();

    // When the screen re-renders with fresh closures for the same data
    view.rerender(
      <Probe onBack={() => {}} onInfo={() => {}} onWrap={() => {}} />
    );

    // Then the options are the same object
    expect(version()).toBe(first);

    // When a displayed value changes
    view.rerender(<Probe title="Storage" />);

    // Then the bar is rebuilt
    expect(version()).not.toBe(first);
  });

  it('Given the native bar, When the screen re-renders with new closures and an item is pressed, Then the newest closure ran and the old one did not', async () => {
    // Given the native bar with a first closure
    native();
    const first = vi.fn();
    const latest = vi.fn();
    const view = renderScreen(<Probe onBack={first} />);

    // When the screen re-renders with a new closure and the back item is pressed
    view.rerender(<Probe onBack={latest} />);
    await userEvent.click(screen.getByTestId('NativeHeaderItem-Go back'));

    // Then only the newest ran
    expect(latest).toHaveBeenCalledTimes(1);
    expect(first).not.toHaveBeenCalled();
  });

  it('Given the native bar, When a menu row that keeps the menu open is pressed, Then the handler ran once and the menu is still open', async () => {
    // Given the native bar and its open settings menu
    native();
    const onWrap = vi.fn();
    renderScreen(<Probe onWrap={onWrap} />);
    await userEvent.click(
      screen.getByTestId('NativeHeaderItem-Editor settings')
    );
    const menu = await waitFor(() =>
      screen.getByTestId('NativeHeaderMenu-Editor settings')
    );

    // When Line wrap is pressed
    await userEvent.click(
      within(menu).getByTestId('NativeHeaderMenuItem-Line wrap')
    );

    // Then the handler ran once and the menu stays open
    expect(onWrap).toHaveBeenCalledTimes(1);
    expect(
      screen.getByTestId('NativeHeaderMenu-Editor settings')
    ).not.toBeNull();
  });

  it('Given the native bar and a title with a subtitle, When it mounts, Then one glass capsule in the bar title holds both lines', () => {
    // Given the native bar
    native();

    // When it mounts
    renderScreen(<Probe />);

    // Then the title is a glass capsule with the title and the subtitle in it
    const capsule = screen.getByTestId('ScreenHeaderGlassTitle');
    expect(capsule.getAttribute('data-glass-style')).toBe('regular');
    expect(within(capsule).getByText('Appearance')).not.toBeNull();
    expect(within(capsule).getByText('Settings')).not.toBeNull();
    expect(screen.getByTestId('NativeHeaderTitle').contains(capsule)).toBe(
      true
    );
  });

  it('Given a bar with more controls on the right, When the title is drawn, Then its capsule is narrower so the title stays centred', () => {
    // Given the capsule of a bar with two controls on the right, on a 393 wide window
    native();
    setWindowWidth(393);
    const view = renderScreen(<Probe />);
    const widest = parseFloat(
      window.getComputedStyle(screen.getByTestId('ScreenHeaderGlassTitle'))
        .maxWidth
    );

    // When the bar has four controls there
    view.rerender(<Probe crowded />);

    // Then the capsule leaves more room for them
    const narrower = parseFloat(
      window.getComputedStyle(screen.getByTestId('ScreenHeaderGlassTitle'))
        .maxWidth
    );
    expect(widest).toBeGreaterThan(96);
    expect(narrower).toBeLessThan(widest);
  });

  it('Given the native bar, When it mounts, Then the toast host is told the bar is the status bar inset plus the bar height', () => {
    // Given the native bar on an iPhone 15 Pro's insets (59 at the top)
    native();

    // When it mounts
    renderScreen(<Probe />);

    // Then the published height is the inset and the 44 of the bar
    expect(useHeaderHeightStore.getState().height).toBe(59 + 44);
  });

  it('Given the navigator placement on the native bar, When the screen mounts, Then only the navigator is told: the twin shows the bar and nothing draws in the body', () => {
    // Given the native bar and a screen that declares its header to the navigator
    native();

    // When the screen mounts
    renderScreen(
      <>
        <ScreenHeader {...bar({})} placement="navigator" />
        <Text testID="Body">Body</Text>
      </>
    );

    // Then the twin has the bar, the Tamagui header is nowhere and the body is what the screen drew
    expect(screen.getByTestId('NativeHeaderItem-Go back')).not.toBeNull();
    expect(screen.queryByTestId('Bar')).toBeNull();
    expect(screen.getByTestId('Body')).not.toBeNull();
  });

  it('Given the navigator placement off the native bar, When the screen mounts, Then nothing draws in the body and the navigator gets the header function that draws the Tamagui header', () => {
    // Given the web environment, with the navigator's headers drawn
    setStackHeadersRendered(true);

    // When the screen mounts with the navigator placement
    renderScreen(
      <>
        <ScreenHeader {...bar({})} placement="navigator" />
        <Text testID="Body">Body</Text>
      </>
    );

    // Then the Tamagui header is the navigator's `header`, drawn once and by it
    expect(screen.getAllByTestId('Bar')).toHaveLength(1);
    expect(screen.getByTestId('BarBack')).not.toBeNull();
    expect(screen.queryByTestId('NativeHeader')).toBeNull();
  });

  it('Given the options hook in the native environment, When the data stays and then changes, Then it returns the same options object and then a new one', () => {
    // Given the native environment and a layout that asks for the options
    native();
    const { result, rerender } = renderHook(
      (props: ProbeProps) => useScreenHeaderOptions(bar(props)),
      { wrapper: FlowProviders, initialProps: {} as ProbeProps }
    );
    const first = result.current;

    // When it renders again with new closures for the same data
    rerender({ onBack: () => {}, onInfo: () => {}, onWrap: () => {} });

    // Then the options are the same object
    expect(result.current).toBe(first);

    // When a displayed value changes
    rerender({ title: 'Storage' });

    // Then they are a new one, still the native bar's
    expect(result.current).not.toBe(first);
    expect(result.current.headerShown).toBe(true);
    expect(typeof result.current.unstable_headerRightItems).toBe('function');
    expect(result.current.header).toBeUndefined();
  });

  it('Given the options hook off the native bar, When it is read, Then the options carry only the header function of the Tamagui header', () => {
    // Given the web environment
    // When a layout asks for the options
    const { result } = renderHook(() => useScreenHeaderOptions(bar({})), {
      wrapper: FlowProviders,
    });

    // Then there is a header function and none of the native keys
    expect(typeof result.current.header).toBe('function');
    expect(result.current.unstable_headerRightItems).toBeUndefined();
    expect(result.current.headerShown).toBeUndefined();
  });
});
