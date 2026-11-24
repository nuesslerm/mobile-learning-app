import type { ErrorCheckpoint, ErrorJump, ErrorPlace } from '../editor/errorJump';

/** The three editor calls a run needs; the editor input's handle satisfies it structurally. */
export type RunEditor = {
  checkpoint: () => Promise<ErrorCheckpoint | null>;
  showErrorLine: (checkpoint: ErrorCheckpoint, place: ErrorPlace | undefined) => Promise<ErrorJump>;
  clearErrorLine: () => Promise<void>;
};
