export type LeetCodeFlags = { aiEdit: boolean };

// Voice edit, typed AI edit and the Plan tab stay built and tested, but their UI entry points are off while the editor fundamentals come first.
// On: EXPO_PUBLIC_LEETCODE_AI_EDIT=1 in the build, or flags: { aiEdit: true } in a flow test's leetcode overrides.
export const DEFAULT_LEETCODE_FLAGS: LeetCodeFlags = { aiEdit: false };

function booleanFromEnv(value: string | undefined, fallback: boolean): boolean {
  if (value === '1') return true;
  if (value === '0') return false;
  return fallback;
}

export function resolveLeetCodeFlags(
  env: { aiEdit?: string },
  overrides: Partial<LeetCodeFlags> = {}
): LeetCodeFlags {
  return {
    aiEdit: overrides.aiEdit ?? booleanFromEnv(env.aiEdit, DEFAULT_LEETCODE_FLAGS.aiEdit),
  };
}
