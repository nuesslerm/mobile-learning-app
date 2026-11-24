import { Logger } from '@lib/logging';
import { useCallback, useEffect, useRef, useState } from 'react';

import type { BlockEditor } from '../../domain/blocks/blockEditor';
import type { BlockModel } from '../../domain/blocks/blockModel';
import { createRefresher, type Refresher } from '../../domain/blocks/latestWins';
import { readBlockModel, type BlockRead } from '../../domain/blocks/readBlockModel';

export type BlockModelStatus = 'waiting' | 'ready' | 'unavailable' | 'unsupported' | 'failed';

export type BlockModelState = {
  readonly status: BlockModelStatus;
  readonly model: BlockModel | null;
};

export type UseBlockModel = BlockModelState & {
  /** reads the editor again, after the one in flight if there is one */
  refresh: () => void;
  /** keeps the model as it is, whatever the editor does, until `release`: a drag in flight must not see its rows change under it */
  hold: () => void;
  /** lets the newest model read while held through */
  release: () => void;
};

const WAITING: BlockModelState = { status: 'waiting', model: null };

/** A read that gave no model leaves the one on show; where there is none, it says why. */
function next(previous: BlockModelState, read: BlockRead): BlockModelState {
  switch (read.kind) {
    case 'ready':
      return { status: 'ready', model: read.model };
    case 'unavailable':
      return { status: 'unavailable', model: null };
    case 'unsupported':
      return { status: 'unsupported', model: null };
    case 'not-ready':
      return previous.model ? previous : WAITING;
    case 'stale':
    case 'failed':
      return previous.model ? previous : { status: 'failed', model: null };
  }
}

/**
 * The block model of an editor, read when the editor says its text changed or that a page started, and held back while a drag is in flight. Reads are coalesced: a burst of changes costs the read in flight
 * and one more. Nothing is read while `editor` is null (the page is not ready).
 */
export function useBlockModel(editor: BlockEditor | null): UseBlockModel {
  const [state, setState] = useState<BlockModelState>(WAITING);
  const heldRef = useRef(false);
  const pendingRef = useRef<BlockRead | null>(null);
  const refresherRef = useRef<Refresher | null>(null);

  useEffect(() => {
    if (!editor) {
      setState((previous) => (previous === WAITING ? previous : WAITING));
      return undefined;
    }
    const refresher = createRefresher(
      async () => {
        await editor.settled();
        const started = Date.now();
        const read = await readBlockModel(editor.client);
        Logger.info('leetcode.blocks.read', {
          kind: read.kind,
          ms: Date.now() - started,
          ...(read.kind === 'ready' ? { rows: read.model.rows.length, revision: read.model.revision, complete: read.model.complete } : {}),
        });
        return read;
      },
      (read) => {
        if (heldRef.current) pendingRef.current = read;
        else setState((previous) => next(previous, read));
      }
    );
    refresherRef.current = refresher;
    const offChanged = editor.client.on('changed', () => refresher.request());
    const offReady = editor.client.on('ready', () => refresher.request());
    refresher.request();
    return () => {
      offChanged();
      offReady();
      refresher.stop();
      refresherRef.current = null;
      pendingRef.current = null;
    };
  }, [editor]);

  const refresh = useCallback(() => refresherRef.current?.request(), []);
  const hold = useCallback(() => {
    heldRef.current = true;
  }, []);
  const release = useCallback(() => {
    heldRef.current = false;
    const read = pendingRef.current;
    pendingRef.current = null;
    if (read) setState((previous) => next(previous, read));
  }, []);

  return { ...state, refresh, hold, release };
}
