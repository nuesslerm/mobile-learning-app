import { Logger } from '@lib/logging';
import AsyncStorage from '@react-native-async-storage/async-storage';
import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';
/**
 * LeetCode auth state — a read-only cache of what leetcode-backend last
 * reported, refreshed by LoginScreen's `GET /user-status` probe (on mount and
 * on focus). Neither platform stores a session cookie any more —
 * backend/leetcode-backend owns the session server-side — so this store is purely display state, not credentials.
 *
 * `clearCredentials` resets the local display cache (wired to the "Log out"
 * button in ProblemListScreen's AuthHeader) but does NOT revoke the backend
 * session — there is no app-facing route to do that (ADR-006: session
 * mutation requires an admin secret the app never carries). The next
 * successful `user-status` refresh will show "Signed in" again if the
 * backend session is still valid. This is an accepted, documented gap, not a
 * bug.
 *
 * Lives in the feature's `state/` layer: it may depend on `domain/`, but UI
 * and integration layers depend on it (not the other way around).
 */
type LeetCodeAuthState = {
  isLoggedIn: boolean;
  username: string | null;
  setStatus: (isSignedIn: boolean, username: string | null) => void;
  clearCredentials: () => void;
};
export const useLeetCodeAuthStore = create<LeetCodeAuthState>()(
  persist(
    (set) => ({
      isLoggedIn: false,
      username: null,
      setStatus: (isSignedIn, username) => {
        Logger.info('leetcode.auth.status', {
          isSignedIn,
          hasUsername: username != null,
        });
        set({
          isLoggedIn: isSignedIn,
          username: isSignedIn ? username : null,
        });
      },
      clearCredentials: () => {
        Logger.info('leetcode.auth.clear', {});
        set({
          isLoggedIn: false,
          username: null,
        });
      },
    }),
    {
      name: 'leetcode-auth',
      // v1 persisted a `webSessionCookie` field this shape no longer has.
      // Bumping the version with no `migrate` function makes zustand log a
      // (harmless, library-internal) console.error and fall back to the
      // fresh default state on the one hydration where an old-shape blob is
      // still on disk, rather than merging a stray unused key into the
      // store.
      version: 2,
      storage: createJSONStorage(() => AsyncStorage),
      partialize: (state) => ({
        isLoggedIn: state.isLoggedIn,
        username: state.username,
      }),
      onRehydrateStorage: () => (_state, err) => {
        if (err)
          Logger.withError(err).error('leetcode.auth.rehydrate_failed');
      },
    }
  )
);
