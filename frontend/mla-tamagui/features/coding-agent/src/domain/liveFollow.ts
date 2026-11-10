// features/coding-agent/src/domain/liveFollow.ts
//
// Pure state machine for the thread feed's auto-scroll ("live follow") flag.
// Ported from references/t3code/apps/mobile/src/features/threads/thread-feed-live-follow.ts
// `resolveThreadFeedLiveFollow`: no `effect`, no IO. The flag is `true` while
// the feed should stick to the bottom as new entries arrive; a real user
// scroll pauses it, programmatic layout compensation does not.

export type ThreadFeedLiveFollowEvent =
  | { readonly type: 'reset' }
  | { readonly type: 'user-scroll-begin' }
  | {
      readonly type: 'user-scroll-end';
      readonly isAtEnd: boolean;
      readonly userScrollSessionActive: boolean;
    }
  | {
      readonly type: 'scroll';
      readonly isAtEnd: boolean;
      readonly userScrollSessionActive: boolean;
    };

/**
 * Resolve the next live-follow flag from the current flag and an incoming
 * feed event. `reset` re-arms at the bottom; `user-scroll-begin` pauses
 * immediately; `user-scroll-end` re-arms only when the user has finished a
 * real scroll at the bottom; `scroll` keeps the flag on while the feed
 * compensates for layout changes and re-arms at the bottom when no user
 * scroll is active.
 */
export function resolveThreadFeedLiveFollow(
  current: boolean,
  event: ThreadFeedLiveFollowEvent,
): boolean {
  switch (event.type) {
    case 'reset':
      return true;
    case 'user-scroll-begin':
      return false;
    case 'user-scroll-end':
      return event.userScrollSessionActive ? event.isAtEnd : current;
    case 'scroll':
      if (event.userScrollSessionActive) {
        return false;
      }
      if (event.isAtEnd) {
        return true;
      }
      return current;
  }
}