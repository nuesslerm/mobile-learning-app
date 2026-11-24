import { router, type Href } from 'expo-router';

/**
 * Back out of a settings subpage. `router.back()` alone is a no-op when the
 * page was reached by deep link or cold start (no history entry to pop) --
 * the inert back button the samples shipped. Fall back to `replace` so the
 * fallback target does not stay in the stack behind the page being left.
 */
export function backTo(href: Href): void {
  if (router.canGoBack()) {
    router.back();
    return;
  }
  router.replace(href);
}
