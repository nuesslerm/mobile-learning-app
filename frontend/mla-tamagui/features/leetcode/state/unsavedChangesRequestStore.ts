import { createOverlayRequestStore } from '@lib/tamagui-design-system';

export type UnsavedChangesRequest = {
  problemId: string;
  /** Leave the editor, discarding the buffer. A snapshot is taken first. */
  onDiscard: () => void;
  /** Stay put. */
  onCancel: () => void;
};

export const useUnsavedChangesRequestStore =
  createOverlayRequestStore<UnsavedChangesRequest>('leetcode.unsavedChanges');
