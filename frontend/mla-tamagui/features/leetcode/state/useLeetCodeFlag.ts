import { useContext } from 'react';

import { resolveLeetCodeFlags, type LeetCodeFlags } from '../domain/featureFlags';
import { LeetCodePortsContext } from './ports';

// Screens outside the leetcode container (the problem list) read the same flags from the build env.
export function useLeetCodeFlag(name: keyof LeetCodeFlags): boolean {
  const ports = useContext(LeetCodePortsContext);
  const flags =
    ports?.flags ??
    resolveLeetCodeFlags({
      aiEdit: process.env.EXPO_PUBLIC_LEETCODE_AI_EDIT,
      cursorPad: process.env.EXPO_PUBLIC_LEETCODE_CURSOR_PAD,
    });
  return flags[name];
}
