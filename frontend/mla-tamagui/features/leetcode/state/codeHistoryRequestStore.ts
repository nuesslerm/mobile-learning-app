import { createOverlayRequestStore } from '@lib/tamagui-design-system';

export type CodeHistoryRequest = {
  problemId: string;
  /** Applies the chosen snapshot's code back into the live editor buffer. */
  onRestore: (code: string, language: string) => void;
};

export const useCodeHistoryRequestStore =
  createOverlayRequestStore<CodeHistoryRequest>('leetcode.codeHistory');
