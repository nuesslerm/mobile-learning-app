import { applyCommands } from '@lib/voice-edit-dsl';
import type { EditAlternative } from '@lib/voice-edit-dsl';

export interface VoiceEditApplication {
  originalText: string;
  modifiedText: string;
  selection: { start: number; end: number };
  diffRange: { start: number; end: number };
}

export function applyVoiceEdit(
  originalText: string,
  alternative: EditAlternative
): VoiceEditApplication {
  const result = applyCommands(originalText, alternative.commands);
  const diffRange = computeDiffRange(originalText, result.text);

  return {
    originalText,
    modifiedText: result.text,
    selection: result.selection,
    diffRange,
  };
}

function computeDiffRange(
  original: string,
  modified: string
): { start: number; end: number } {
  const prefix = commonPrefixLength(original, modified);
  const suffix = commonSuffixLength(original, modified, prefix);
  return {
    start: prefix,
    end: modified.length - suffix,
  };
}

function commonPrefixLength(a: string, b: string): number {
  const max = Math.min(a.length, b.length);
  let i = 0;
  while (i < max && a[i] === b[i]) {
    i++;
  }
  return i;
}

function commonSuffixLength(a: string, b: string, prefix: number): number {
  const max = Math.min(a.length - prefix, b.length - prefix);
  if (max <= 0) return 0;
  let i = 0;
  while (i < max && a[a.length - 1 - i] === b[b.length - 1 - i]) {
    i++;
  }
  return i;
}
