import { Logger } from '@lib/logging';
import React from 'react';

import type { ErrorCheckpoint, ErrorPlace } from '../../domain/editor/errorJump';
import type { RunEditor } from '../../domain/run/runEditor';

export type JumpResult = { kind: 'shown'; line: number } | { kind: 'missed' } | { kind: 'none' } | { kind: 'stale' };

export type ErrorJump = {
  /** Marks the reported line of the checkpoint. A mark that lands after `clear` is taken away again. */
  show: (checkpoint: ErrorCheckpoint, place: ErrorPlace | undefined) => Promise<JumpResult>;
  /** Marks the last shown line once more ("Line N"). */
  showAgain: () => Promise<JumpResult>;
  clear: () => void;
};

/** The E2 jump controller: `token` moves whenever the result goes, `live` says a mark was asked for, `last` is what "Line N" repeats. */
export function createErrorJump(editor: () => RunEditor | null | undefined): ErrorJump {
  let token = 0;
  let live = false;
  let last: { checkpoint: ErrorCheckpoint; place: ErrorPlace } | null = null;
  return {
    async show(checkpoint, place) {
      const mine = token;
      if (place) {
        live = true;
        last = { checkpoint, place };
      }
      const outcome = await editor()?.showErrorLine(checkpoint, place);
      if (!place) return { kind: 'none' };
      if (token !== mine) {
        void editor()?.clearErrorLine();
        return { kind: 'stale' };
      }
      return outcome?.kind === 'shown' ? { kind: 'shown', line: outcome.line } : { kind: 'missed' };
    },
    async showAgain() {
      const mine = token;
      const again = last;
      if (!again) return { kind: 'none' };
      Logger.info('editor_v2.run_error.line_tap', { line: again.place.line });
      const outcome = await editor()?.showErrorLine(again.checkpoint, again.place);
      if (token !== mine) return { kind: 'stale' };
      return outcome?.kind === 'shown' ? { kind: 'shown', line: outcome.line } : { kind: 'missed' };
    },
    clear() {
      token += 1;
      last = null;
      if (!live) return;
      live = false;
      void editor()?.clearErrorLine();
    },
  };
}

export function useErrorJump(editorRef: React.RefObject<RunEditor | null>): ErrorJump {
  const ref = React.useRef<ErrorJump | null>(null);
  ref.current ??= createErrorJump(() => editorRef.current);
  return ref.current;
}
