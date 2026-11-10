import { Slot, Stack, useLocalSearchParams, useNavigation, usePathname } from 'expo-router';
import type { Context } from 'react';
import { Pressable, Text, View } from 'react-native';
import { describe, expect, it } from 'vitest';

import { screen, userEvent } from '../index';
import { renderRoute } from '../route';

function OuterLayout() {
  return (
    <View testID="layout-outer">
      <Stack />
    </View>
  );
}

function InnerLayout() {
  return (
    <View testID="layout-inner">
      <Stack />
    </View>
  );
}

function IdScreen() {
  const { id } = useLocalSearchParams<{ id: string }>();
  return (
    <View testID="screen">
      <Text testID="id">{id}</Text>
    </View>
  );
}

function SlotLayout() {
  return (
    <View testID="layout-slot">
      <Slot />
    </View>
  );
}

function PathnameScreen() {
  return <Text testID="pathname">{usePathname()}</Text>;
}

function MenuScreen() {
  const navigation = useNavigation();
  const parent = navigation.getParent();
  return (
    <Pressable testID="menu" onPress={() => parent?.dispatch({ type: 'OPEN_DRAWER' })}>
      <Text testID="has-parent">{parent ? 'drawer' : 'none'}</Text>
    </Pressable>
  );
}

type DrawerNavigation = { dispatch(action: { type: string }): void };
const drawerNavigation = (): Context<DrawerNavigation | undefined> =>
  (globalThis as { __flowDrawerNavigation?: Context<DrawerNavigation | undefined> }).__flowDrawerNavigation!;

describe('renderRoute', () => {
  it('given two Stack layouts and a screen reading the id param, when mounted via renderRoute, then both wrappers and the id render', () => {
    renderRoute({
      layouts: [OuterLayout, InnerLayout],
      screen: IdScreen,
      params: { id: 'two-sum' },
      pathname: '/leetcode/two-sum',
    });
    expect(screen.getByTestId('layout-outer')).toBeTruthy();
    expect(screen.getByTestId('layout-inner')).toBeTruthy();
    expect(screen.getByText('two-sum')).toBeTruthy();
  });

  it('given a Slot layout, when mounted via renderRoute, then the slot renders the screen', () => {
    renderRoute({ layouts: [SlotLayout], screen: IdScreen, params: { id: 'two-sum' } });
    expect(screen.getByTestId('layout-slot')).toBeTruthy();
    expect(screen.getByText('two-sum')).toBeTruthy();
  });

  it('given a pathname, when the screen reads usePathname, then it returns the seeded pathname', () => {
    renderRoute({ layouts: [], screen: PathnameScreen, pathname: '/leetcode/three-sum' });
    expect(screen.getByTestId('pathname').textContent).toBe('/leetcode/three-sum');
  });

  it('given a screen with no drawer above it, when it asks for its navigation parent, then there is none', () => {
    renderRoute({ layouts: [], screen: MenuScreen });
    expect(screen.getByTestId('has-parent').textContent).toBe('none');
  });

  it('given a screen inside a drawer layout, when it dispatches OPEN_DRAWER to its parent, then the drawer receives it', async () => {
    const actions: string[] = [];
    const DrawerLayout = () => {
      const Provider = drawerNavigation().Provider;
      return (
        <Provider value={{ dispatch: (action) => actions.push(action.type) }}>
          <Slot />
        </Provider>
      );
    };
    renderRoute({ layouts: [DrawerLayout], screen: MenuScreen });
    expect(screen.getByTestId('has-parent').textContent).toBe('drawer');

    await userEvent.click(screen.getByTestId('menu'));

    expect(actions).toEqual(['OPEN_DRAWER']);
  });
});
