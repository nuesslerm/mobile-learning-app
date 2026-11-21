import type { TestCase } from '../../domain/run/execution';
import type { Harness, NodeType } from '../../domain/models';

/** What the page's `judge` call takes: the learner's code, the cases, and what the harness needs to build and compare values. */
export type JudgePayload = {
  code: string;
  methodName: string;
  testCases: TestCase[];
  paramTypes?: Record<string, NodeType>;
  returnType?: NodeType | null;
  inPlace?: string | null;
  harness?: Harness | null;
  /** the limit of one case (and of the module-level code), as the backend's 5 s */
  timeoutMs?: number;
  /** the limit of the whole run: past it the harness stops with the backend's process-limit message */
  processLimitMs?: number;
};
export type JudgeParams = { runId: number; payload: JudgePayload };
/** One finished case, emitted as the run goes, so a crash that kills the interpreter still leaves the rows before it. */
export type JudgeCaseEvent = { runId: number; row: string };

export const JUDGE_DEFAULTS = { timeoutMs: 5000, processLimitMs: 30_000, rpcSlackMs: 10_000 } as const;
