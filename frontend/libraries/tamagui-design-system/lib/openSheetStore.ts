import { create } from 'zustand';

type OpenSheetState = {
  count: number;
  open: () => void;
  close: () => void;
};

/**
 * How many sheets are currently open, app-wide.
 *
 * It exists for one thing the components cannot settle between themselves: the
 * keyboard toolbar rail is a BODY-LEVEL portal at `position: fixed`, while a
 * sheet deliberately renders inside its route and never portals. No z-index set
 * from inside a route can outrank a portal that escaped it -- so the rail
 * painted over an open sheet and swallowed the taps meant for it. A count the
 * rail can read is the only thing that actually removes the overlap, and it is
 * the right behaviour anyway: a modal sheet owns the screen while it is up.
 */
export const useOpenSheetStore = create<OpenSheetState>()((set) => ({
  count: 0,
  open: () => set((s) => ({ count: s.count + 1 })),
  close: () => set((s) => ({ count: Math.max(0, s.count - 1) })),
}));
