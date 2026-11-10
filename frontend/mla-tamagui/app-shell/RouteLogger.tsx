import { Logger } from '@lib/logging';
import { usePathname } from 'expo-router';
import { useEffect, useRef } from 'react';

// Its own component so a navigation re-renders this and not the root layout's whole tree.
// The first route logs with `from` undefined: there is no earlier route to name as where the move came from.
export function RouteLogger() {
  const pathname = usePathname();
  const previous = useRef<string | undefined>(undefined);
  useEffect(() => {
    Logger.info('route.change', { to: pathname, from: previous.current });
    previous.current = pathname;
  }, [pathname]);
  return null;
}
