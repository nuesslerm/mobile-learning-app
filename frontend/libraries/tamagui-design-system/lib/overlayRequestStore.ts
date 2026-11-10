import { Logger } from '@lib/logging';
import { create } from 'zustand';

type OverlayRequestState<TPayload> = {
  payload: TPayload | null;
  request: (payload: TPayload) => void;
  clear: () => void;
};

/**
 * Cross-instance request channel for a router-addressable overlay.
 *
 * Once an overlay is its own route, the screen that triggers it no longer
 * shares a React tree with it — an imperative ref can't reach across that
 * boundary. Call `request(payload)` (bundling any onConfirm/onCancel
 * callbacks the invoking screen needs into the payload) immediately before
 * `router.push`-ing the overlay route; the overlay route reads the same
 * payload back out via the hook this returns. See
 * `docs/overlays-as-routes.md` for the full recipe.
 */
export function createOverlayRequestStore<TPayload>(name: string) {
  return create<OverlayRequestState<TPayload>>()((set) => ({
    payload: null,
    request: (payload) => {
      Logger.info('overlay_request.request', { name });
      set({ payload });
    },
    clear: () => {
      Logger.info('overlay_request.clear', { name });
      set({ payload: null });
    },
  }));
}
