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
// <Stack> children are route declarations (Stack.Screen), not content: a real
// layout like app/(app)/(content)/_layout.tsx names every child screen and the
// slot must still mount the current route. Ignore children for Stack.
const renderStack = () => (g.__flowRouter.screen ? React.createElement(g.__flowRouter.screen) : null);
// A screen inside the expo-router/drawer stub has the drawer as its navigation parent; drawer actions bubble up to it.
g.__flowDrawerNavigation = g.__flowDrawerNavigation ?? React.createContext(undefined);
const useNavigation = () => {
  const drawer = React.useContext(g.__flowDrawerNavigation);
  return { setOptions() {}, addListener: () => () => {}, getParent: () => drawer, dispatch: (action) => drawer?.dispatch(action) };
};
module.exports = { __esModule: true, router, useRouter: () => router, useLocalSearchParams: useParams, useGlobalSearchParams: useParams, usePathname: () => g.__flowRouter.pathname ?? '/', useSegments: () => [], useFocusEffect: (fn) => React.useEffect(() => { const c = fn(); return typeof c === 'function' ? c : undefined; }, []), useNavigation,Link: (p) => React.createElement('a', { href: String(p.href), 'data-testid': p.testID }, p.children), Stack: Object.assign(renderStack, { Screen: () => null }), Slot: renderScreenSlot, Redirect: () => null };
