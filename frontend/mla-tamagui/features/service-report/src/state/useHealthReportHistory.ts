/**
 * Loads the health-report history from the ORDS client on mount and exposes
 * a `refresh()` to re-run it (wired to the screen's refresh button). Plain
 * component state rather than a zustand store — this data is scoped to one
 * screen and isn't shared or persisted across the app, unlike e.g.
 * leetcode's `authStore`.
 */

import { useDependencyOverrides } from '@lib/app-di';
import { Logger } from '@lib/logging';
import type { HealthReport } from '../domain';
import { useCallback, useEffect, useState } from 'react';

import { fetchHealthReportHistory } from '../data/ordsClient';


type HealthReportHistoryState =
  | { status: 'loading' }
  | { status: 'not-configured'; reason: string }
  | { status: 'unreachable'; error: string }
  | { status: 'ready'; reports: HealthReport[] };

export type UseHealthReportHistoryResult = HealthReportHistoryState & {
  refresh: () => void;
};

export function useHealthReportHistory(): UseHealthReportHistoryResult {
  const [state, setState] = useState<HealthReportHistoryState>({
    status: 'loading',
  });

  const { fetchHealthReportHistory: overriddenFetch } =
    useDependencyOverrides<{
      fetchHealthReportHistory: typeof fetchHealthReportHistory;
    }>('service-report');

  const refresh = useCallback(() => {
    setState({ status: 'loading' });
    (overriddenFetch ?? fetchHealthReportHistory)()
      .then((result) => {
        if (result.status === 'ready') {
          setState({ status: 'ready', reports: result.reports });
        } else {
          setState(result);
        }
      })
      .catch((err: unknown) => {
        Logger.error('service_report.history_load_failed', { err });
        setState({
          status: 'unreachable',
          error: err instanceof Error ? err.message : String(err),
        });
      });
  }, [overriddenFetch]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  return { ...state, refresh };
}
