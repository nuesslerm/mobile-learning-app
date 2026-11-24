import {
  DarkTheme,
  DefaultTheme,
  Slot,
  Stack,
  ThemeProvider,
  useLocalSearchParams,
  useNavigation,
  usePathname,
} from 'expo-router';
import type { Context } from 'react';
import { Pressable, Text, View } from 'react-native';
import { describe, expect, it } from 'vitest';

import {
  navigationTheme,
  resetRouterHarness,
  screen,
  setStackHeadersRendered,
  userEvent,
  within,
} from '../index';
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

function HeaderScreen() {
  return (
    <View testID="body">
      <Stack.Screen options={{ header: () => <Text testID="screen-header">Two Sum</Text> }} />
      <Text testID="content">content</Text>
    </View>
  );
}

function LoginLayout() {
  return (
    <View testID="layout-login">
      <Stack>
        <Stack.Screen name="[id]/description" options={{ header: () => <Text testID="description-header">Description</Text> }} />
        <Stack.Screen name="login" options={{ header: () => <Text testID="login-header">Sign in</Text> }} />
        <Stack.Screen name="toolbar" options={{ headerShown: false, header: () => <Text testID="toolbar-header">Toolbar</Text> }} />
      </Stack>
    </View>
  );
}

function PlainScreen() {
  return <Text testID="content">content</Text>;
}

describe('expo-router stub headers', () => {
  it('given the default, when a screen declares a header through Stack.Screen, then nothing renders for it', () => {
    renderRoute({ layouts: [], screen: HeaderScreen });
    expect(screen.getByTestId('content')).toBeTruthy();
    expect(screen.queryByTestId('screen-header')).toBeNull();
  });

  it('given headers are rendered, when a screen declares a header through Stack.Screen, then it renders where the Stack.Screen sits', () => {
    setStackHeadersRendered(true);

    renderRoute({ layouts: [], screen: HeaderScreen });

    expect(within(screen.getByTestId('body')).getByTestId('screen-header').textContent).toBe('Two Sum');
  });

  it('given headers are rendered, when the options hide the header or are a function of the route, then it follows them', () => {
    setStackHeadersRendered(true);
    const Hidden = () => (
      <View>
        <Stack.Screen options={{ headerShown: false, header: () => <Text testID="hidden-header" /> }} />
        <Stack.Screen name="late" options={({ route }) => ({ header: () => <Text testID="route-header">{route.name}</Text> })} />
      </View>
    );

    renderRoute({ layouts: [], screen: Hidden });

    expect(screen.queryByTestId('hidden-header')).toBeNull();
    expect(screen.getByTestId('route-header').textContent).toBe('late');
  });

  it('given headers are rendered and a layout names a screen whose name ends the pathname, when the layout mounts, then that header renders above the slot', () => {
    setStackHeadersRendered(true);

    renderRoute({ layouts: [LoginLayout], screen: PlainScreen, pathname: '/leetcode/login' });

    const header = screen.getByTestId('login-header');
    const content = screen.getByTestId('content');
    expect(header.compareDocumentPosition(content) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(screen.queryByTestId('description-header')).toBeNull();
  });

  it('given a layout names a dynamic screen, when the pathname fills its segment, then its header renders', () => {
    setStackHeadersRendered(true);

    renderRoute({ layouts: [LoginLayout], screen: PlainScreen, pathname: '/leetcode/two-sum/description' });

    expect(screen.getByTestId('description-header')).toBeTruthy();
    expect(screen.queryByTestId('login-header')).toBeNull();
  });

  it('given a layout names no screen for the pathname or one with headerShown false, when it mounts, then no header renders', () => {
    setStackHeadersRendered(true);

    renderRoute({ layouts: [LoginLayout], screen: PlainScreen, pathname: '/leetcode/toolbar' });

    expect(screen.queryByTestId('toolbar-header')).toBeNull();
    expect(screen.getByTestId('content')).toBeTruthy();
  });

  it('given the default, when a layout names a screen for the pathname, then its header does not render', () => {
    renderRoute({ layouts: [LoginLayout], screen: PlainScreen, pathname: '/leetcode/login' });

    expect(screen.queryByTestId('login-header')).toBeNull();
  });

  it('given the flag and a recorded theme, when the router harness resets, then both are back to their defaults', () => {
    setStackHeadersRendered(true);
    renderRoute({ layouts: [], screen: () => <ThemeProvider value={DarkTheme}><Text>x</Text></ThemeProvider> });
    expect(navigationTheme()?.dark).toBe(true);

    resetRouterHarness();
    renderRoute({ layouts: [], screen: HeaderScreen });

    expect(navigationTheme()).toBeUndefined();
    expect(screen.queryByTestId('screen-header')).toBeNull();
  });
});

describe('expo-router stub theme', () => {
  const Themed = ({ value }: { value: typeof DarkTheme }) => (
    <>
      <ThemeProvider value={value}>
        <Text testID="inside">inside</Text>
      </ThemeProvider>
      <Text testID="outside">outside</Text>
    </>
  );

  it('given a ThemeProvider with DarkTheme, when it mounts, then its children render with no extra element and the harness reports a dark theme', () => {
    renderRoute({ layouts: [], screen: () => <Themed value={DarkTheme} /> });

    expect(screen.getByTestId('inside').parentElement).toBe(screen.getByTestId('outside').parentElement);
    expect(navigationTheme()?.dark).toBe(true);
  });

  it('given a ThemeProvider with DefaultTheme, when it mounts, then the harness reports a light theme and nothing before it did', () => {
    expect(navigationTheme()).toBeUndefined();

    renderRoute({ layouts: [], screen: () => <Themed value={DefaultTheme} /> });

    expect(navigationTheme()?.dark).toBe(false);
  });
});
