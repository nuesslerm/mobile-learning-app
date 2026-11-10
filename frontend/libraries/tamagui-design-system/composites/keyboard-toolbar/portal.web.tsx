import type { ReactNode } from 'react';
import { createPortal } from 'react-dom';

/**
 * Web: hoist the rail to `document.body`.
 *
 * Not ceremony -- it is the only thing that gets the rail out of the
 * navigator's stacking context. The rail lives several levels inside a
 * react-navigation screen container, and a z-index only orders siblings WITHIN
 * the nearest stacking context: raising the rail to z 2000 in there still
 * leaves the whole container below a later sibling at z 1999 in the root
 * context. Observed exactly that way -- the debug FAB kept swallowing taps on
 * the rail's right-hand keys (Rename, Done) no matter how the two z values
 * were set, until the rail was portalled out.
 */
export function KeyboardToolbarPortal({ children }: { children: ReactNode }) {
  if (typeof document === 'undefined') return <>{children}</>;
  return createPortal(children, document.body);
}
