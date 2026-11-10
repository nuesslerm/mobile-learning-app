// features/coding-agent/src/domain/__tests__/liveFollow.test.ts
//
// Ported in structure from
// references/t3code/apps/mobile/src/features/threads/thread-feed-live-follow.test.ts
// onto our `resolveThreadFeedLiveFollow` / `ThreadFeedLiveFollowEvent` types
// (the same switch, no `effect`, no IO). The reference's
// `resolveThreadFeedSubmissionAnchor` cases are dropped because that helper
// does not exist in this port. Every test is Given/When/Then.

import { describe, expect, it } from 'vitest';

import {
  resolveThreadFeedLiveFollow,
  type ThreadFeedLiveFollowEvent,
} from '../liveFollow';

describe('resolveThreadFeedLiveFollow', () => {
  it('given a user-scroll-begin while following, when resolved, then the flag pauses immediately', () => {
    // Given the flag is following and a user-scroll-begin event arrives
    const event: ThreadFeedLiveFollowEvent = { type: 'user-scroll-begin' };
    // When resolved
    const next = resolveThreadFeedLiveFollow(true, event);
    // Then the flag pauses
    expect(next).toBe(false);
  });

  it('given a scroll away from the end with a user scroll session, when resolved, then the flag stays paused', () => {
    // Given the flag is paused and a scroll event from a user gesture lands away from the end
    const event: ThreadFeedLiveFollowEvent = {
      type: 'scroll',
      isAtEnd: false,
      userScrollSessionActive: true,
    };
    // When resolved
    const next = resolveThreadFeedLiveFollow(false, event);
    // Then the flag stays paused
    expect(next).toBe(false);
  });

  it('given a scroll not at the end without a user scroll session, when resolved, then the flag is not disturbed by layout compensation', () => {
    // Given the flag is following and a programmatic layout-compensation scroll fires
    const event: ThreadFeedLiveFollowEvent = {
      type: 'scroll',
      isAtEnd: false,
      userScrollSessionActive: false,
    };
    // When resolved
    const next = resolveThreadFeedLiveFollow(true, event);
    // Then the flag stays following
    expect(next).toBe(true);
  });

  it('given a scroll at the end with a user scroll session, when resolved, then the flag does not re-arm mid-gesture', () => {
    // Given the flag is paused and the user scrolls back to the end mid-gesture
    const event: ThreadFeedLiveFollowEvent = {
      type: 'scroll',
      isAtEnd: true,
      userScrollSessionActive: true,
    };
    // When resolved
    const next = resolveThreadFeedLiveFollow(false, event);
    // Then the flag stays paused
    expect(next).toBe(false);
  });

  it('given a user-scroll-end with a user scroll session, when resolved, then the flag re-arms only when the user landed at the end', () => {
    // Given a user-scroll-end event with an active user session
    const atEnd: ThreadFeedLiveFollowEvent = {
      type: 'user-scroll-end',
      isAtEnd: true,
      userScrollSessionActive: true,
    };
    const awayFromEnd: ThreadFeedLiveFollowEvent = {
      type: 'user-scroll-end',
      isAtEnd: false,
      userScrollSessionActive: true,
    };
    // When resolved
    const rearmed = resolveThreadFeedLiveFollow(false, atEnd);
    const stayedPaused = resolveThreadFeedLiveFollow(false, awayFromEnd);
    // Then the flag re-arms only when the user finished at the end
    expect(rearmed).toBe(true);
    expect(stayedPaused).toBe(false);
  });

  it('given a user-scroll-end with no user scroll session, when resolved, then the flag ignores the programmatic momentum event', () => {
    // Given the flag is following and a programmatic user-scroll-end fires
    const event: ThreadFeedLiveFollowEvent = {
      type: 'user-scroll-end',
      isAtEnd: false,
      userScrollSessionActive: false,
    };
    // When resolved
    const next = resolveThreadFeedLiveFollow(true, event);
    // Then the flag stays following
    expect(next).toBe(true);
  });

  it('given a reset event, when resolved, then the flag re-arms regardless of the current value', () => {
    // Given the flag is paused and a reset event arrives
    const event: ThreadFeedLiveFollowEvent = { type: 'reset' };
    // When resolved
    const next = resolveThreadFeedLiveFollow(false, event);
    // Then the flag re-arms
    expect(next).toBe(true);
  });
});
