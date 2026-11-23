export type LeetCodeFlags = { aiEdit: boolean; cursorPad: boolean };

// Voice edit, typed AI edit and the Plan tab stay built and tested, but their UI entry points are off while the editor fundamentals come first.
// On: EXPO_PUBLIC_LEETCODE_AI_EDIT=1 in the build, or flags: { aiEdit: true } in a flow test's leetcode overrides.
// The cursor pad on the keyboard rail is built and tested but its entry points are off too (owner's call, 23 Nov: it does not earn its place yet).
// On: EXPO_PUBLIC_LEETCODE_CURSOR_PAD=1 in the build, or flags: { cursorPad: true } in a flow test's leetcode overrides.
export const DEFAULT_LEETCODE_FLAGS: LeetCodeFlags = { aiEdit: false, cursorPad: false };

function booleanFromEnv(value: string | undefined, fallback: boolean): boolean {
  if (value === '1') return true;
  if (value === '0') return false;
  return fallback;
}

export function resolveLeetCodeFlags(
  env: { aiEdit?: string; cursorPad?: string },
  overrides: Partial<LeetCodeFlags> = {}
): LeetCodeFlags {
  return {
    aiEdit: overrides.aiEdit ?? booleanFromEnv(env.aiEdit, DEFAULT_LEETCODE_FLAGS.aiEdit),
    cursorPad: overrides.cursorPad ?? booleanFromEnv(env.cursorPad, DEFAULT_LEETCODE_FLAGS.cursorPad),
  };
}
