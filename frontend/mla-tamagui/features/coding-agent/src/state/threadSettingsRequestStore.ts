import { createOverlayRequestStore } from '@lib/tamagui-design-system';

// Cross-instance request channel for the per-thread settings overlay.
// `ThreadSettingsSheet` is router-addressable (`app/coding-agent/[threadId]/
// settings.tsx`), so the screen that triggers it cannot reach the overlay
// with an imperative ref — the wiring follows the leetcode pattern from
// `frontend/mla-tamagui/features/leetcode/state/{renameSymbolRequestStore,resetCodeRequestStore}.ts`,
// where the request payload carries the routing id (`problemId` there,
// `threadId` here) so the overlay route knows which thread to render the
// settings for. The store itself is a single-payload channel (matching
// the `createOverlayRequestStore` shape): only one settings sheet can be
// open at a time.
export type ThreadSettingsRequest = {
  threadId: string;
};

export const useThreadSettingsRequestStore =
  createOverlayRequestStore<ThreadSettingsRequest>('coding-agent.threadSettings');