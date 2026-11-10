// expo-router/drawer's navigator is expo-router's runtime and its vendored react-navigation ships Flow syntax jsdom
// cannot parse. The drawer itself is real: react-native-drawer-layout, the view expo-router's DrawerView renders,
// resolved from the package's own expo-router (flowTestConfig) and closed until a screen or the content opens it.
import React from 'react';
import { ScrollView, StyleSheet } from 'react-native';
import { Drawer as DrawerLayout } from 'react-native-drawer-layout';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

const g = globalThis;
g.__flowDrawerNavigation ??= React.createContext(undefined);
const DrawerNavigationContext = g.__flowDrawerNavigation;

const DRAWER_ACTIONS = {
  OPEN_DRAWER: () => true,
  CLOSE_DRAWER: () => false,
  TOGGLE_DRAWER: (open) => !open,
};

const noop = () => undefined;

export function Drawer({ drawerContent, screenOptions }) {
  const [open, setOpen] = React.useState(false);
  const navigation = React.useMemo(() => {
    const dispatch = (action) => {
      const next = DRAWER_ACTIONS[action?.type];
      if (next) setOpen(next);
    };
    return {
      dispatch,
      openDrawer: () => dispatch({ type: 'OPEN_DRAWER' }),
      closeDrawer: () => dispatch({ type: 'CLOSE_DRAWER' }),
      toggleDrawer: () => dispatch({ type: 'TOGGLE_DRAWER' }),
      getParent: () => undefined,
      setOptions: noop,
      addListener: () => noop,
      isFocused: () => true,
    };
  }, []);
  const options = (typeof screenOptions === 'function' ? screenOptions({}) : screenOptions) ?? {};
  const state = { key: 'drawer', index: 0, routes: [{ key: '(content)', name: '(content)' }], history: [] };
  const screen = g.__flowRouter?.screen ? React.createElement(g.__flowRouter.screen) : null;

  return React.createElement(
    DrawerNavigationContext.Provider,
    { value: navigation },
    React.createElement(
      DrawerLayout,
      {
        open,
        onOpen: navigation.openDrawer,
        onClose: navigation.closeDrawer,
        drawerType: options.drawerType ?? 'front',
        drawerPosition: options.drawerPosition ?? 'left',
        drawerStyle: options.drawerStyle,
        overlayStyle: { backgroundColor: options.overlayColor ?? 'rgba(0, 0, 0, 0.5)' },
        overlayAccessibilityLabel: options.overlayAccessibilityLabel,
        // The export's `body { overflow: hidden }` is the phone's screen edge: the content slid aside goes off it.
        style: styles.screenEdge,
        renderDrawerContent: () =>
          drawerContent ? drawerContent({ state, navigation, descriptors: {} }) : null,
      },
      screen
    )
  );
}
Drawer.Screen = function DrawerScreen() {
  return null;
};

const SPACING = 12;

// The vendored DrawerContentScrollView needs a NavigationContainer for its text direction; this is its body for ltr.
export function DrawerContentScrollView({ contentContainerStyle, style, children, ...rest }) {
  const insets = useSafeAreaInsets();
  return React.createElement(
    ScrollView,
    {
      ...rest,
      contentContainerStyle: [
        {
          paddingTop: SPACING + insets.top,
          paddingBottom: SPACING + insets.bottom,
          paddingStart: SPACING + insets.left,
          paddingEnd: SPACING,
        },
        contentContainerStyle,
      ],
      style: [styles.container, style],
    },
    children
  );
}

const styles = StyleSheet.create({ container: { flex: 1 }, screenEdge: { overflow: 'hidden' } });

export default { Drawer, DrawerContentScrollView };
