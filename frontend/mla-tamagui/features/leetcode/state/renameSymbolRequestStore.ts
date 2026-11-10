import { createOverlayRequestStore } from '@lib/tamagui-design-system';

export type RenameSymbolRequest = {
  problemId: string;
  target: string;
  onConfirm: (newName: string) => void;
  onCancel: () => void;
};

export const useRenameSymbolRequestStore =
  createOverlayRequestStore<RenameSymbolRequest>('leetcode.renameSymbol');
