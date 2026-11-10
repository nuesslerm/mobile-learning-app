// features/coding-agent/src/state/dispatchRunsStore.ts
//
// Zustand store that owns the dispatch-runs section state: the list
// summary (`runs`), the per-run detail transcripts (`details`), the
// global `status` and `error`, and three explicit setters plus two async
// loaders that round-trip through a `DispatchRunsGateway`. `loadRuns` and
// `loadRun` set `status: 'loading'` before the call, `'ready'` on
// resolution and `'error'` (with `error: message`) on rejection while
// leaving the previous `runs` / `details` in place so the UI can keep
// showing the last good snapshot while the banner flips to error.
import { Logger } from '@lib/logging';
import type {
  DispatchRunDetail,
  DispatchRunMeta,
} from '../domain';
import { create } from 'zustand';

import type { DispatchRunsGateway } from '../data/dispatchRunsGateway';

export type DispatchRunsStatus = 'idle' | 'loading' | 'ready' | 'error';
type DispatchRunsListFilter = { agent?: string; q?: string };

type DispatchRunsStoreState = {
  runs: DispatchRunMeta[];
  details: Record<string, DispatchRunDetail>;
  status: DispatchRunsStatus;
  error?: string;
  setRuns: (runs: DispatchRunMeta[]) => void;
  setDetail: (runId: string, detail: DispatchRunDetail) => void;
  setStatus: (status: DispatchRunsStatus, error?: string) => void;
  loadRuns: (
    gateway: DispatchRunsGateway,
    filter?: DispatchRunsListFilter,
  ) => Promise<void>;
  loadRun: (gateway: DispatchRunsGateway, runId: string) => Promise<void>;
};

// Routine polls stay silent: only the first success after idle or an error is news.
const isFirstSuccess = (before: DispatchRunsStatus): boolean =>
  before === 'idle' || before === 'error';

export const useDispatchRunsStore = create<DispatchRunsStoreState>((set, get) => ({
  runs: [],
  details: {},
  status: 'idle',
  setRuns: (runs) => {
    set({ runs });
  },
  setDetail: (runId, detail) => {
    set((state) => ({
      details: { ...state.details, [runId]: detail },
    }));
  },
  setStatus: (status, error) => {
    set({ status, error });
  },
  loadRuns: async (gateway, filter) => {
    const { status: before, runs: previous } = get();
    const startedAt = Date.now();
    set({ status: 'loading' });
    try {
      const runs = await gateway.listRuns(filter);
      set({ runs, status: 'ready', error: undefined });
      if (isFirstSuccess(before) || runs.length !== previous.length) {
        Logger.info('coding-agent.dispatch_runs.loaded', {
          op: 'listRuns',
          count: runs.length,
          ms: Date.now() - startedAt,
        });
      }
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause);
      // The gateway already warned with op, url and status; this line would repeat it on every poll.
      Logger.withError(cause).withMetadata({ op: 'loadRuns' }).debug(
        'coding-agent.store.dispatch_runs.load_runs_failed',
      );
      set({ status: 'error', error: message });
    }
  },
  loadRun: async (gateway, runId) => {
    const { status: before, details } = get();
    const startedAt = Date.now();
    set({ status: 'loading' });
    try {
      const detail = await gateway.getRun(runId);
      set((state) => ({
        details: { ...state.details, [runId]: detail },
        status: 'ready',
        error: undefined,
      }));
      if (isFirstSuccess(before) || details[runId]?.status !== detail.status) {
        Logger.info('coding-agent.dispatch_runs.loaded', {
          op: 'getRun',
          runId,
          status: detail.status,
          ms: Date.now() - startedAt,
        });
      }
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause);
      Logger.withError(cause)
        .withMetadata({ op: 'loadRun', runId })
        .debug('coding-agent.store.dispatch_runs.load_run_failed');
      set({ status: 'error', error: message });
    }
  },
}));