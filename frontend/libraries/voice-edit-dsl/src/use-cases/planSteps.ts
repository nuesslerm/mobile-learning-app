import {
  lastNonBlankLineIndex,
  trailingStubLine,
  type RealizeRequest,
} from './pseudoCode';

export type PlanStepStatus = 'pending' | 'realized' | 'stale';

/** How the step's words arrived, so its realise call is measured as what it was. */
export type PlanStepOrigin = 'voice' | 'text';

export type PlanStep = {
  id: string;
  text: string;
  status: PlanStepStatus;
  lines: { startLine: number; endLine: number } | null;
  origin: PlanStepOrigin;
};

export type PlanAnchor =
  | { kind: 'replaceStub'; line: number }
  | { kind: 'insertAfterLine'; line: number };

/** A spoken or typed block becomes one step per sentence; a terminator-less block is one step. */
export function splitBlock(text: string): string[] {
  return text
    .split(/[.?!\n]/)
    .map((part) => part.trim())
    .filter((part) => part.length > 0);
}

/**
 * Where the step at `index` lands: after the nearest realised earlier step,
 * else over the trailing whitespace-only stub line, else after the last
 * non-blank line.
 */
export function planAnchor(
  code: string,
  steps: PlanStep[],
  index: number
): PlanAnchor {
  // Only the immediately preceding step anchors this one. Scanning further
  // back would put step N where step N-1 is still going to land, so the code
  // would end up in realise order rather than plan order.
  const previous = index > 0 ? steps[index - 1] : undefined;
  if (previous?.status === 'realized' && previous.lines !== null)
    return { kind: 'insertAfterLine', line: previous.lines.endLine };
  const stub = trailingStubLine(code);
  if (stub !== null) return { kind: 'replaceStub', line: stub };
  return { kind: 'insertAfterLine', line: lastNonBlankLineIndex(code) + 1 };
}

export function buildStepRealizeRequest(
  code: string,
  language: string,
  step: PlanStep,
  anchor: PlanAnchor
): RealizeRequest {
  const startLine = anchor.line;
  const endLine = anchor.line;
  const transcript =
    anchor.kind === 'replaceStub'
      ? `Replace the stub on line ${anchor.line} with the real ${language} code for: "${step.text}". Do not add, remove or change any other line.`
      : `Insert new lines after line ${anchor.line} implementing: "${step.text}". Do not change line ${anchor.line} or any other line.`;
  return { startLine, endLine, transcript, label: 'Implement step' };
}

/** The lines an applied edit added at the anchor, by line-count diff; null when nothing was added. */
export function linesProduced(
  before: string,
  after: string,
  anchor: PlanAnchor
): { startLine: number; endLine: number } | null {
  if (after === before) return null;
  const added = after.split('\n').length - before.split('\n').length;
  if (anchor.kind === 'replaceStub') {
    if (added < 0) return null;
    return { startLine: anchor.line, endLine: anchor.line + added };
  }
  if (added <= 0) return null;
  return { startLine: anchor.line + 1, endLine: anchor.line + added };
}
