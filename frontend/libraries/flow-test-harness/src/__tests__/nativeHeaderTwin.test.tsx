import {
  Stack,
  type NativeStackHeaderItemMenuAction,
  type NativeStackHeaderItemMenuSubmenu,
  type NativeStackNavigationOptions,
} from 'expo-router';
import { useState } from 'react';
import { Text, View } from 'react-native';
import { describe, expect, it, vi } from 'vitest';

import { act, screen, setStackHeadersRendered, userEvent, within } from '../index';
import { renderRoute } from '../route';

type Options = NativeStackNavigationOptions;
type Row = NativeStackHeaderItemMenuAction | NativeStackHeaderItemMenuSubmenu;

function mountOptions(options: Options) {
  setStackHeadersRendered(true);
  renderRoute({
    layouts: [],
    screen: () => (
      <View>
        <Stack.Screen options={options} />
        <Text testID="body">body</Text>
      </View>
    ),
  });
}

function menuOf(items: Row[], label = 'Settings'): Options {
  return {
    unstable_headerRightItems: () => [
      { type: 'menu', label, sharesBackground: false, menu: { multiselectable: true, items } },
    ],
  };
}

function MenuProbe({ onPress }: { onPress: (name: string) => void }) {
  const [wrap, setWrap] = useState(true);
  const [size, setSize] = useState(14);
  const sizes = [12, 14, 16];
  return (
    <View>
      <Stack.Screen
        options={{
          unstable_headerRightItems: () => [
            {
              type: 'menu',
              label: 'Settings',
              menu: {
                multiselectable: true,
                items: [
                  {
                    type: 'submenu',
                    label: 'Display',
                    inline: true,
                    items: [
                      {
                        type: 'submenu',
                        label: `Font size: ${size}px`,
                        multiselectable: true,
                        items: sizes.map((s) => ({
                          type: 'action' as const,
                          label: `${s}px`,
                          state: s === size ? ('on' as const) : ('off' as const),
                          keepsMenuPresented: true,
                          onPress: () => {
                            onPress(`size ${s}`);
                            setSize(s);
                          },
                        })),
                      },
                      {
                        type: 'action',
                        label: 'Line wrap',
                        state: wrap ? 'on' : 'off',
                        keepsMenuPresented: true,
                        onPress: () => {
                          onPress('wrap');
                          setWrap((w) => !w);
                        },
                      },
                    ],
                  },
                  { type: 'action', label: 'Symbol Bar', onPress: () => onPress('symbols') },
                ],
              },
            },
          ],
        }}
      />
    </View>
  );
}

describe('native header twin', () => {
  it('given items with grouping flags and no header, when headers are rendered, then each item is a button with its flags, spacing is a gap and a custom item shows its element', async () => {
    const back = vi.fn();
    const info = vi.fn();
    mountOptions({
      headerShown: true,
      headerTransparent: false,
      unstable_headerLeftItems: () => [
        { type: 'button', label: 'Back', accessibilityLabel: 'Back to the list', icon: { type: 'sfSymbol', name: 'chevron.left' }, sharesBackground: false, onPress: back },
      ],
      unstable_headerRightItems: () => [
        { type: 'custom', element: <Text testID="busy">busy</Text>, hidesSharedBackground: true },
        { type: 'spacing', spacing: 12 },
        { type: 'button', label: 'Info', sharesBackground: true, onPress: info },
      ],
    });

    const left = screen.getByTestId('NativeHeaderLeft');
    const right = screen.getByTestId('NativeHeaderRight');
    const backItem = within(left).getByTestId('NativeHeaderItem-Back to the list');
    expect(backItem.getAttribute('data-shares-background')).toBe('false');
    expect(backItem.getAttribute('data-icon')).toBe('chevron.left');
    expect(within(right).getByTestId('NativeHeaderCustom').getAttribute('data-hides-shared-background')).toBe('true');
    expect(within(within(right).getByTestId('NativeHeaderCustom')).getByTestId('busy')).toBeTruthy();
    expect(within(right).getByTestId('NativeHeaderSpacing').getAttribute('data-spacing')).toBe('12');
    expect(within(right).getByTestId('NativeHeaderItem-Info').getAttribute('data-shares-background')).toBe('true');
    const order = Array.from(right.children).map((child) => child.getAttribute('data-testid'));
    expect(order).toEqual(['NativeHeaderCustom', 'NativeHeaderSpacing', 'NativeHeaderItem-Info']);
    expect(screen.getByTestId('NativeHeader').getAttribute('data-header-shown')).toBe('true');
    expect(screen.getByTestId('NativeHeader').getAttribute('data-header-transparent')).toBe('false');

    await userEvent.click(backItem);
    await userEvent.click(within(right).getByTestId('NativeHeaderItem-Info'));

    expect(back).toHaveBeenCalledTimes(1);
    expect(info).toHaveBeenCalledTimes(1);
  });

  it('given a header function next to items, when headers are rendered, then the function wins and no bar draws', () => {
    mountOptions({
      header: () => <Text testID="custom-header">custom</Text>,
      unstable_headerRightItems: () => [{ type: 'button', label: 'Info', onPress: () => undefined }],
    });
    expect(screen.getByTestId('custom-header')).toBeTruthy();
    expect(screen.queryByTestId('NativeHeader')).toBeNull();
  });

  it('given headerShown false with items, when headers are rendered, then no bar draws', () => {
    mountOptions({ headerShown: false, unstable_headerRightItems: () => [{ type: 'button', label: 'Info', onPress: () => undefined }] });

    expect(screen.queryByTestId('NativeHeader')).toBeNull();
  });

  it('given a headerTitle function and a title, when headers are rendered, then the bar shows what the function returns for that title', () => {
    mountOptions({ title: 'Two Sum', headerTitle: ({ children }) => <Text testID="capsule">{`capsule ${children}`}</Text> });

    expect(within(screen.getByTestId('NativeHeaderTitle')).getByTestId('capsule').textContent).toBe('capsule Two Sum');
  });

  it('given a bare title and no items, when headers are rendered, then no bar draws because that is the stock bar', () => {
    mountOptions({ title: 'Toolbar' });

    expect(screen.queryByTestId('NativeHeader')).toBeNull();
  });

  it('given a menu item, when it is pressed, then a menu lists its rows and a row with a state is a checkbox with aria-checked', async () => {
    mountOptions(
      menuOf([
        { type: 'action', label: 'Show Whitespace', state: 'off', onPress: () => undefined },
        { type: 'action', label: 'Line wrap', state: 'on', onPress: () => undefined },
        { type: 'action', label: 'Symbol Bar', onPress: () => undefined },
        { type: 'action', label: 'Skipped', hidden: true, onPress: () => undefined },
      ])
    );
    expect(screen.queryByRole('menu')).toBeNull();

    await userEvent.click(screen.getByTestId('NativeHeaderItem-Settings'));

    const menu = screen.getByRole('menu');
    expect(menu.getAttribute('data-multiselectable')).toBe('true');
    expect(within(menu).getByTestId('NativeHeaderMenuItem-Line wrap').getAttribute('role')).toBe('menuitemcheckbox');
    expect(within(menu).getByTestId('NativeHeaderMenuItem-Line wrap').getAttribute('aria-checked')).toBe('true');
    expect(within(menu).getByTestId('NativeHeaderMenuItem-Show Whitespace').getAttribute('aria-checked')).toBe('false');
    expect(within(menu).getByTestId('NativeHeaderMenuItem-Symbol Bar').getAttribute('role')).toBe('menuitem');
    expect(within(menu).queryByTestId('NativeHeaderMenuItem-Skipped')).toBeNull();
  });

  it('given a kept-open action and a plain one, when each is pressed, then onPress ran once and only the plain one closes the menu', async () => {
    const calls: string[] = [];
    setStackHeadersRendered(true);
    renderRoute({ layouts: [], screen: () => <MenuProbe onPress={(name) => calls.push(name)} /> });
    await userEvent.click(screen.getByTestId('NativeHeaderItem-Settings'));

    await userEvent.click(screen.getByTestId('NativeHeaderMenuItem-Line wrap'));

    expect(calls).toEqual(['wrap']);
    expect(screen.getByRole('menu')).toBeTruthy();
    expect(screen.getByTestId('NativeHeaderMenuItem-Line wrap').getAttribute('aria-checked')).toBe('false');

    await userEvent.click(screen.getByTestId('NativeHeaderMenuItem-Symbol Bar'));

    expect(calls).toEqual(['wrap', 'symbols']);
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('given a submenu inside an inline section, when its row is pressed, then the list is replaced in place by a header row and its items, and the header row returns to the parent', async () => {
    const calls: string[] = [];
    setStackHeadersRendered(true);
    renderRoute({ layouts: [], screen: () => <MenuProbe onPress={(name) => calls.push(name)} /> });
    await userEvent.click(screen.getByTestId('NativeHeaderItem-Settings'));
    expect(screen.getByRole('group', { name: 'Display' })).toBeTruthy();

    await userEvent.click(screen.getByTestId('NativeHeaderMenuItem-Font size: 14px'));

    expect(screen.getByTestId('NativeHeaderMenuHeader-Font size: 14px')).toBeTruthy();
    expect(screen.queryByTestId('NativeHeaderMenuItem-Line wrap')).toBeNull();
    expect(screen.getByTestId('NativeHeaderMenuItem-14px').getAttribute('aria-checked')).toBe('true');
    expect(screen.getByTestId('NativeHeaderMenuItem-16px').getAttribute('aria-checked')).toBe('false');

    await userEvent.click(screen.getByTestId('NativeHeaderMenuHeader-Font size: 14px'));

    expect(screen.queryByTestId('NativeHeaderMenuHeader-Font size: 14px')).toBeNull();
    expect(screen.getByTestId('NativeHeaderMenuItem-Line wrap')).toBeTruthy();
    expect(calls).toEqual([]);
  });

  it('given an option picked in a submenu that keeps the menu presented, when it is pressed, then onPress ran once and the menu is back at its root with the new value', async () => {
    const calls: string[] = [];
    setStackHeadersRendered(true);
    renderRoute({ layouts: [], screen: () => <MenuProbe onPress={(name) => calls.push(name)} /> });
    await userEvent.click(screen.getByTestId('NativeHeaderItem-Settings'));
    await userEvent.click(screen.getByTestId('NativeHeaderMenuItem-Font size: 14px'));

    await userEvent.click(screen.getByTestId('NativeHeaderMenuItem-16px'));

    expect(calls).toEqual(['size 16']);
    expect(screen.getByRole('menu')).toBeTruthy();
    expect(screen.queryByTestId('NativeHeaderMenuHeader-Font size: 16px')).toBeNull();
    expect(screen.getByTestId('NativeHeaderMenuItem-Font size: 16px')).toBeTruthy();
  });

  it('given a destructive action with a description and a disabled one, when the menu opens, then the flags and the description show and a press on the disabled one does nothing', async () => {
    const onDisabled = vi.fn();
    mountOptions(
      menuOf([
        { type: 'action', label: 'Format', description: 'Your code will be reformatted.', destructive: true, onPress: () => undefined },
        { type: 'action', label: 'Locked', disabled: true, onPress: onDisabled },
      ])
    );

    await userEvent.click(screen.getByTestId('NativeHeaderItem-Settings'));
    await userEvent.click(screen.getByTestId('NativeHeaderMenuItem-Locked'));

    expect(screen.getByTestId('NativeHeaderMenuItem-Format').getAttribute('data-destructive')).toBe('true');
    expect(screen.getByTestId('NativeHeaderMenuDescription-Format').textContent).toBe('Your code will be reformatted.');
    expect(onDisabled).not.toHaveBeenCalled();
  });

  it('given an open menu, when the item is pressed again, then it closes', async () => {
    mountOptions(menuOf([{ type: 'action', label: 'Symbol Bar', onPress: () => undefined }]));
    await userEvent.click(screen.getByTestId('NativeHeaderItem-Settings'));
    expect(screen.getByRole('menu')).toBeTruthy();

    await userEvent.click(screen.getByTestId('NativeHeaderItem-Settings'));

    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('given a Stack.Screen whose options object is kept and then replaced, when the screen re-renders, then the bar reports a new options version only for the new object', () => {
    const first: Options = { unstable_headerRightItems: () => [{ type: 'button', label: 'Info', onPress: () => undefined }] };
    const held: { options: Options; rerender: () => void } = { options: first, rerender: () => undefined };
    function Screen() {
      const [, setCount] = useState(0);
      held.rerender = () => setCount((count) => count + 1);
      return (
        <View>
          <Stack.Screen options={held.options} />
        </View>
      );
    }
    setStackHeadersRendered(true);
    renderRoute({ layouts: [], screen: Screen });
    const version = () => screen.getByTestId('NativeHeader').getAttribute('data-options-version');
    const initial = version();

    act(() => held.rerender());

    expect(version()).toBe(initial);

    held.options = { ...first };
    act(() => held.rerender());

    expect(Number(version())).toBe(Number(initial) + 1);
  });
});
