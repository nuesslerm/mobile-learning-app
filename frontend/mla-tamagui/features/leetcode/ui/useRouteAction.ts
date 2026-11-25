import { router, useFocusEffect, type Href } from 'expo-router';
import { useCallback, useRef } from 'react';

/** A mounted screen can receive another press before its navigation transition finishes. */
export function useRouteAction() {
  const available = useRef(false);
  useFocusEffect(useCallback(() => {
    available.current = true;
    return () => { available.current = false; };
  }, []));
  return useCallback((action: () => void) => {
    if (!available.current) return;
    available.current = false;
    action();
  }, []);
}

export function useRouteNavigation() {
  const leave = useRouteAction();
  const pushRoute = useCallback((href: Href) => leave(() => router.push(href)), [leave]);
  return { leave, pushRoute };
}
