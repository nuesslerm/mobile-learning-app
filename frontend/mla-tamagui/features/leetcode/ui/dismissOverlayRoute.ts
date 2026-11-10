import { router } from 'expo-router';

/**
 * Dismiss an overlay route (description, history, rename, reset-code, unsaved)
 * back to the editor it belongs to.
 *
 * `router.back()` alone is a no-op when there is no history entry to pop, and
 * an overlay reached by a deep link or a cold start has exactly none -- the
 * user lands with the sheet already open and its close control does nothing,
 * on a screen with no other way out. Found by a check that starts at the
 * overlay route instead of tapping its way there (issue #164): close left the
 * path unchanged at `/leetcode/two-sum/description` with the sheet still
 * mounted and no editor underneath.
 *
 * `replace` rather than `push` for the fallback: the overlay should not stay
 * in the stack behind the editor, or Back from the editor reopens it.
 */
export function dismissOverlayRoute(problemId: string | undefined): void {
  if (router.canGoBack()) {
    router.back();
    return;
  }
  // No `/leetcode` index in this app -- the problem list is `(app)/(content)/index`,
  // i.e. `/`. Naming a route expo-router has not generated is a typed-routes
  // error, not a silent no-op.
  router.replace(problemId ? `/leetcode/${problemId}` : '/');
}
