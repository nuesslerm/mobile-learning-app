import { Logger } from '@lib/logging';
import { useEffect, useRef, useState } from 'react';
import type { Pairing } from '../domain/mobile';
import type { T3MobilePort } from '../domain/t3Mobile';

export function useWorkspaceDisclosure(api: T3MobilePort, pair: Pairing) {
  const [keys, setKeys] = useState<ReadonlySet<string>>(new Set());
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string>();
  const [revision, refresh] = useState(0);
  const session = useRef<AbortController | undefined>(undefined);
  const confirmed = useRef<string[]>([]);
  const pending = useRef(new Map<string, { collapsed: boolean }>());

  function renderKeys() {
    const visible = new Set(confirmed.current);
    for (const [key, change] of pending.current) {
      if (change.collapsed) visible.add(key);
      else visible.delete(key);
    }
    setKeys(visible);
  }

  useEffect(() => {
    const controller = new AbortController();
    session.current = controller;
    pending.current.clear();
    setReady(false);
    setError(undefined);
    api.workspaceDisclosure(pair, controller.signal).then(saved => {
      if (controller.signal.aborted) return;
      confirmed.current = saved;
      renderKeys();
      setReady(true);
    }).catch(cause => {
      if (controller.signal.aborted) return;
      Logger.withError(cause).warn('coding_agent.workspace_disclosure.load_failed');
      setError('Could not load collapsed workspace groups.');
    });
    return () => controller.abort();
  }, [api, pair, revision]);

  async function change(key: string, collapsed: boolean) {
    const controller = session.current;
    if (!ready || !controller || controller.signal.aborted) return;
    const edit = { collapsed };
    pending.current.set(key, edit);
    setError(undefined);
    renderKeys();
    try {
      const saved = await api.setWorkspaceDisclosure(pair, key, collapsed);
      if (!controller.signal.aborted) confirmed.current = saved;
    } catch (cause) {
      Logger.withError(cause).warn('coding_agent.workspace_disclosure.save_failed');
      if (!controller.signal.aborted && pending.current.get(key) === edit) {
        setError('Could not save collapsed workspace groups.');
      }
    } finally {
      if (!controller.signal.aborted) {
        if (pending.current.get(key) === edit) pending.current.delete(key);
        renderKeys();
      }
    }
  }

  return { keys, ready, change, error, reload: () => refresh(n => n + 1) };
}
