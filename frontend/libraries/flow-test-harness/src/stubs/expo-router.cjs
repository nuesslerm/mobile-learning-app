'use strict';
const React = require('react');
const g = globalThis;
g.__flowRouter = g.__flowRouter ?? { calls: [], params: {} };
const subscribe = (notify) => { const r = g.__flowRouter; r.listeners = r.listeners ?? new Set(); r.listeners.add(notify); return () => r.listeners.delete(notify); };
const useParams = () => React.useSyncExternalStore(subscribe, () => g.__flowRouter.params, () => g.__flowRouter.params);
const router = { push: (h) => g.__flowRouter.calls.push(['push', h]), replace: (h) => g.__flowRouter.calls.push(['replace', h]), back: () => g.__flowRouter.calls.push(['back']), navigate: (h) => g.__flowRouter.calls.push(['navigate', h]), canGoBack: () => true, setParams: () => {} };
// A layout's <Stack/>/<Slot/> mounts the current route screen when it has no
// children; renderRoute seeds g.__flowRouter.screen with the nested chain.
const renderScreenSlot = (p) => p.children ?? (g.__flowRouter.screen ? React.createElement(g.__flowRouter.screen) : null);
// A screen inside the expo-router/drawer stub has the drawer as its navigation parent; drawer actions bubble up to it.
g.__flowDrawerNavigation = g.__flowDrawerNavigation ?? React.createContext(undefined);
const useNavigation = () => {
  const drawer = React.useContext(g.__flowDrawerNavigation);
  return { setOptions() {}, addListener: () => () => {}, getParent: () => drawer, dispatch: (action) => drawer?.dispatch(action) };
};

// Opt-in (setStackHeadersRendered): the navigator's bar has no DOM, and turning it on for every test would move the editor-route goldens.
const twin = require('./nativeHeaderTwin.cjs');
const headersRendered = () => g.__flowRouter.stackHeadersRendered === true;
const resolveOptions = (options, props) => (typeof options === 'function' ? options(props) : options) ?? {};
const renderHeader = (options, route, navigation, version) => {
  if (options.headerShown === false) return null;
  if (typeof options.header === 'function') return options.header({ navigation, route, options, back: undefined });
  return twin.renderNativeHeader(options, version);
};
// How many times this screen's options object was replaced: a bar rebuilt from a new object is what recreates an open UIMenu on a phone.
const ScreenOptions = (props) => {
  const navigation = useNavigation();
  const held = React.useRef({ options: undefined, version: 0 });
  if (!headersRendered()) return null;
  const route = { key: props.name ?? 'screen', name: props.name ?? 'screen', params: g.__flowRouter.params };
  const options = resolveOptions(props.options, { route, navigation });
  if (held.current.options !== options) held.current = { options, version: held.current.version + 1 };
  return renderHeader(options, route, navigation, held.current.version);
};
// A name matches the end of the pathname, a [param] segment any one segment; `index` never matches.
const nameMatches = (name, pathname) => {
  const want = name.split('/').filter(Boolean);
  const have = (pathname ?? '').split('?')[0].split('/').filter(Boolean);
  if (want.length === 0 || want.length > have.length) return false;
  return want.every((segment, i) => segment !== 'index' && (/^\[.+\]$/.test(segment) || segment === have[have.length - want.length + i]));
};
const declaredScreens = (children) => React.Children.toArray(children).flatMap((child) => {
  if (!React.isValidElement(child)) return [];
  if (child.type === React.Fragment) return declaredScreens(child.props.children);
  return child.type === ScreenOptions && typeof child.props.name === 'string' ? [child.props] : [];
});
// <Stack> children are route declarations, not content: a layout names every child screen and the slot must still mount
// the current route. With headers on, the declaration matching the pathname draws its header above it.
const StackNavigator = (props) => {
  const navigation = useNavigation();
  const body = g.__flowRouter.screen ? React.createElement(g.__flowRouter.screen) : null;
  if (!headersRendered()) return body;
  const declared = declaredScreens(props.children).find((screen) => nameMatches(screen.name, g.__flowRouter.pathname));
  if (!declared) return body;
  const route = { key: declared.name, name: declared.name, params: g.__flowRouter.params };
  const header = renderHeader(resolveOptions(declared.options, { route, navigation }), route, navigation);
  return header ? React.createElement(React.Fragment, null, header, body) : body;
};

// ThemeProvider records its value instead of wrapping: an element would move every golden that mounts the root.
const DefaultTheme = { dark: false, colors: { primary: 'rgb(0, 122, 255)', background: 'rgb(242, 242, 242)', card: 'rgb(255, 255, 255)', text: 'rgb(28, 28, 30)', border: 'rgb(216, 216, 216)', notification: 'rgb(255, 59, 48)' } };
const DarkTheme = { dark: true, colors: { primary: 'rgb(10, 132, 255)', background: 'rgb(1, 1, 1)', card: 'rgb(18, 18, 18)', text: 'rgb(229, 229, 231)', border: 'rgb(39, 39, 41)', notification: 'rgb(255, 69, 58)' } };
const ThemeProvider = (props) => { g.__flowRouter.theme = props.value; return props.children ?? null; };

module.exports = { __esModule: true, router, useRouter: () => router, useLocalSearchParams: useParams, useGlobalSearchParams: useParams, usePathname: () => g.__flowRouter.pathname ?? '/', useRootNavigationState: () => ({ routes: [] }), useSegments: () => [], useFocusEffect: (fn) => React.useEffect(() => { const c = fn(); return typeof c === 'function' ? c : undefined; }, []), useNavigation,Link: (p) => React.createElement('a', { href: String(p.href), 'data-testid': p.testID }, p.children), Stack: Object.assign(StackNavigator, { Screen: ScreenOptions }), Slot: renderScreenSlot, Redirect: () => null, ThemeProvider, DefaultTheme, DarkTheme };
