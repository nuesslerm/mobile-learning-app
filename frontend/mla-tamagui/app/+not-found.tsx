import { Logger } from '@lib/logging';
import { Unmatched, usePathname } from 'expo-router';
import { useEffect } from 'react';

// expo-router's own "Unmatched Route" screen, plus the line a stale or mistyped deep link would otherwise never leave.
export default function NotFound() {
  const pathname = usePathname();
  useEffect(() => {
    Logger.warn('nav.unmatched', { pathname });
  }, [pathname]);
  return <Unmatched />;
}
