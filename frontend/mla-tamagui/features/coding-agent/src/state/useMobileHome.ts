import { useEffect, useRef, useState } from 'react';
import type { MobilePort, Pairing, Workspace, MobileThread, RepositoryTask } from '../domain/mobile';

export function useMobileHome(api: MobilePort) {
  const [pair, setPair] = useState<Pairing | null>(null);
  const [ready, setReady] = useState(false);
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [selected, setSelected] = useState<string>();
  const [threads, setThreads] = useState<MobileThread[]>([]);
  const [tasks, setTasks] = useState<RepositoryTask[]>([]);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const generation = useRef(0);
  useEffect(() => {
    let active = true;
    api.readPairing().then(saved => { if (active) setPair(saved); })
      .catch(e => { if (active) setError(String(e)); })
      .finally(() => { if (active) setReady(true); });
    return () => { active = false; };
  }, [api]);

  useEffect(() => {
    if (!pair) return;
    let active = true;
    setLoading(true);
    setError('');
    setThreads([]);
    setTasks([]);
    const reads = selected ? [
      api.threads(pair, selected).then(rows => { if (active) setThreads(rows); }),
      api.tasks(pair, selected).then(rows => { if (active) setTasks(rows); }),
    ] : [api.workspaces(pair).then(rows => { if (active) setWorkspaces(rows); })];
    void Promise.allSettled(reads).then(results => {
      if (!active) return;
      const failure = results.find(r => r.status === 'rejected');
      if (failure?.status === 'rejected') setError(String(failure.reason));
      setLoading(false);
    });
    return () => { active = false; };
  }, [api, pair, selected]);

  const busy = useRef(false);
  async function perform(action: () => Promise<void>) {
    if (busy.current) return;
    busy.current = true;
    setError('');
    const request = ++generation.current;
    try { await action(); }
    catch (e) { if (request === generation.current) setError(String(e)); }
    finally { busy.current = false; }
  }
  return {
    pair, ready, workspaces, selected, select: setSelected, threads, tasks, error, loading, perform,
    setConnection: setPair,
    disconnect: () => perform(async () => { if (pair) await api.revoke(pair); setSelected(undefined); setPair(null); }),
  };
}
