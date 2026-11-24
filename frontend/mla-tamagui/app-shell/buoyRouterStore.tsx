import { RouterConfigContext } from 'expo-router/build/global-state/routerConfigContext';
import { useContext, useLayoutEffect, type ContextType } from 'react';

type RouteNode = NonNullable<ContextType<typeof RouterConfigContext>>['routeNode'];

// Buoy reads the pre-SDK-58 router singleton. The live route tree now comes from context.
export const store: { routeNode: RouteNode } = { routeNode: null };

export function BuoyRouterStore() {
  const routeNode = useContext(RouterConfigContext)?.routeNode ?? null;

  useLayoutEffect(() => {
    store.routeNode = routeNode;
    return () => {
      if (store.routeNode === routeNode) store.routeNode = null;
    };
  }, [routeNode]);

  return null;
}
