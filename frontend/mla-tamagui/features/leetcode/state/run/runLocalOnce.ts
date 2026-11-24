import { Logger } from '@lib/logging';

import type { ErrorCheckpoint } from '../../domain/editor/errorJump';
import type { CodeRunnerPort, RunCodeParams } from '../../domain/ports';
import type { RunResponse } from '../../domain/run/execution';
import type { RunEditor } from '../../domain/run/runEditor';

export type RunLocalOutcome =
  | { kind: 'answered'; params: RunCodeParams; checkpoint: ErrorCheckpoint | null; response: RunResponse; ms: number }
  | { kind: 'discarded'; params: RunCodeParams; ms: number }
  | { kind: 'failed'; error: unknown; ms: number };

export type RunLocalInput = {
  runner: CodeRunnerPort;
  editor: Pick<RunEditor, 'checkpoint'> | null | undefined;
  given: RunCodeParams;
  /** Whether the editor still holds the code and language the run was made on. */
  isCurrent: (params: RunCodeParams) => boolean;
  problemId: string | undefined;
  mode: string;
  now?: () => number;
};

/**
 * One run: the editor checkpoint's text (the screen's when the editor is silent), the runner, and the stale rule (an answer for code that
 * has since moved is discarded). Cards and toasts stay with the caller.
 */
export async function runLocalOnce({ runner, editor, given, isCurrent, problemId, mode, now = Date.now }: RunLocalInput): Promise<RunLocalOutcome> {
  const startedAt = now();
  try {
    const checkpoint = (await editor?.checkpoint()) ?? null;
    const params = checkpoint ? { ...given, code: checkpoint.text } : given;
    const response = await runner.run(params);
    const ms = now() - startedAt;
    if (!isCurrent(params)) {
      Logger.info('editor_v2.run_local.discarded', { problemId, mode, lang: params.lang, ms });
      return { kind: 'discarded', params, ms };
    }
    return { kind: 'answered', params, checkpoint, response, ms };
  } catch (error) {
    const ms = now() - startedAt;
    Logger.withError(error).withMetadata({ problemId, mode, lang: given.lang, ms }).error('editor_v2.run_local.error');
    return { kind: 'failed', error, ms };
  }
}
