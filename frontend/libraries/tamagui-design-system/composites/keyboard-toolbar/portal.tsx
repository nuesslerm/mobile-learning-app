import type { ReactNode } from 'react';

/**
 * Native: render in place.
 *
 * An absolutely-positioned view inside the root view already paints above its
 * siblings there, so there is nothing to escape. The web variant is the one
 * that does real work -- see portal.web.tsx.
 */
export function KeyboardToolbarPortal({ children }: { children: ReactNode }) {
  return <>{children}</>;
}
