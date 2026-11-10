import { createOverlayRequestStore } from '@lib/tamagui-design-system';

export type ResetCodeRequest = {
  problemId: string;
  onConfirm: () => void;
  onCancel: () => void;
};

export const useResetCodeRequestStore =
  createOverlayRequestStore<ResetCodeRequest>('leetcode.resetCode');
