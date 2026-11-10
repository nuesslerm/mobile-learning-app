import type { EditAlternative } from '@lib/voice-edit-dsl';

const AUTO_APPLY_THRESHOLD = 0.85;

/**
 * Decides whether a transcribed voice edit should be applied automatically
 * (a single valid alternative, or a clear high-confidence winner) versus
 * shown to the user as a picker. Pure — no IO, no React.
 */
export function selectAutoApplyAlternative(
  alternatives: EditAlternative[]
): EditAlternative | null {
  const valid = alternatives.filter((a) => a.valid !== false);
  const top = valid[0];
  const shouldAutoApply =
    valid.length === 1 ||
    (top != null &&
      top.confidence >= AUTO_APPLY_THRESHOLD &&
      valid.every((a) => a === top || a.confidence < top.confidence - 0.1));

  return shouldAutoApply && top ? top : null;
}
