import type { Harness, NodeType } from '../../domain/models';
import type { TestCase, TestResult } from '../../domain/run/execution';

/** The names the composed script and the harness agree on: symbol keys, so no name is left in the learner's global scope. */
export const CONFIG_KEY = 'mla.leetcode.config';
export const LOADED_KEY = 'mla.leetcode.loaded';

/** What a run tells the harness, written on line 1 of the worker's script. */
export type HarnessConfig = {
  methodName: string;
  paramTypes: Record<string, NodeType>;
  returnType: NodeType | null;
  inPlace: string | null;
  harness: Harness | null;
  /** the cap of one case's output stream; the judge's 256 KiB when left out */
  outputCap?: number;
};

/** A finished case. `stack` is the raised error's stack, for the page to locate: it never leaves the engine. */
export type CaseRow = TestResult & { stack?: string };

export type ToWorker = { type: 'case'; index: number; case: TestCase };
export type FromWorker =
  /** the learner's module-level code has run: its output so far, and why the run cannot start (no callable) */
  | { type: 'loaded'; stdout: string; error?: string }
  /** a case is about to run: the page arms its deadline here */
  | { type: 'begin'; index: number }
  | { type: 'row'; index: number; row: CaseRow };

/** What a worker's ErrorEvent says, in the form every spawn adapter normalizes to. */
export type WorkerError = { message: string; line?: number; column?: number };
