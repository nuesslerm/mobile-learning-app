import { useCallback, useEffect, useRef, useState } from 'react';
import { AppState } from 'react-native';
import { Logger } from '@lib/logging';
import type { Pairing } from '../../domain/mobile';
import type {
  TabAction,
  WorkspaceLayout,
  WorkspaceTabsPort,
} from '../../domain/workspaceTabs/tabs';

export function useWorkspaceTabs(
  api: WorkspaceTabsPort,
  pair: Pairing,
  checkout: string
) {
  const [layout, setLayout] = useState<WorkspaceLayout>();
  const [error, setError] = useState<Error>();
  const [busy, setBusy] = useState(false);
  const [foreground, setForeground] = useState(
    AppState.currentState !== 'background'
  );
  const epoch = useRef(0);
  const inflight = useRef(false);
  const run = useCallback(
    async (action?: TabAction) => {
      if (action && inflight.current) return;
      if (action) inflight.current = true;
      const token = ++epoch.current;
      setBusy(true);
      try {
        const next = await api.layout(pair, checkout, action);
        if (token === epoch.current) {
          setLayout(next);
          setError(undefined);
        }
        return next;
      } catch (cause) {
        Logger.withError(cause).warn('coding_agent.workspace_tabs_failed');
        if (token === epoch.current)
          setError(
            cause instanceof Error
              ? cause
              : new Error('Could not load workspace tabs.')
          );
      } finally {
        if (action) inflight.current = false;
        if (token === epoch.current) setBusy(false);
      }
    },
    [api, pair, checkout]
  );
  useEffect(() => {
    if (foreground) void run();
    return () => {
      epoch.current++;
    };
  }, [run, foreground]);
  useEffect(() => {
    const subscription = AppState.addEventListener('change', (state) =>
      setForeground(state === 'active')
    );
    return () => subscription.remove();
  }, []);
  return { layout, error, busy, run, foreground };
}
