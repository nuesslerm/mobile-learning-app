import { Logger } from '@lib/logging';
import { useCallback, useEffect, useRef, useState } from 'react';
import type { Pairing } from '../domain/mobile';
import type {
  T3Conversation,
  T3MobilePort,
  T3Shell,
  T3Snapshot,
} from '../domain/t3Mobile';

export function useT3Mobile(api: T3MobilePort, threadId?: string) {
  const [pair, setPair] = useState<Pairing | null>();
  const [shell, setShell] = useState<T3Shell>();
  const [snapshot, setSnapshot] = useState<T3Snapshot>();
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [connected, setConnected] = useState(false);
  const connection = useRef<T3Conversation | null>(null);
  const [generation, setGeneration] = useState(0);
  const [draft, setDraft] = useState('');
  const pending = useRef<{ text: string; id: string } | null>(null);
  const sending = useRef(false);
  const retry = useCallback(() => setGeneration((v) => v + 1), []);

  useEffect(() => {
    let active = true;
    api
      .restore()
      .then((saved) => {
        if (active) setPair(saved);
      })
      .catch(() => {
        if (active) {
          setPair(null);
          setError('Could not load the saved connection.');
        }
      });
    return () => {
      active = false;
    };
  }, [api]);

  useEffect(() => {
    if (!pair) return;
    let active = true;
    let current: T3Conversation | undefined;
    let timer: ReturnType<typeof setTimeout>;
    let failures = 0;
    function disconnected(reason: Error) {
      if (!active) return;
      setConnected(false);
      setError(reason.message);
      timer = setTimeout(
        () => void open(),
        Math.min(1000 * 2 ** failures++, 15000)
      );
    }
    async function open() {
      if (!pair || !active) return;
      try {
        if (!threadId) {
          const result = await api.shell(pair);
          if (active) {
            setShell(result);
            setError('');
          }
          return;
        }
        const session = await api.open(
          pair,
          threadId,
          (result) => {
            if (!active) return;
            failures = 0;
            setSnapshot(result);
            setConnected(true);
            setError('');
          },
          disconnected
        );
        if (!active) {
          session.close();
          return;
        }
        current = session;
        connection.current = session;
      } catch (e) {
        Logger.warn('coding_agent.t3.connection_failed');
        disconnected(e instanceof Error ? e : new Error('Connection failed.'));
      }
    }
    void open();
    return () => {
      active = false;
      clearTimeout(timer);
      current?.close();
      connection.current = null;
    };
  }, [pair, api, threadId, generation]);

  async function connect(host: string, code: string) {
    if (busy) return;
    setBusy(true);
    try {
      setPair(await api.pair(host, code));
      setError('');
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Pairing failed.');
    } finally {
      setBusy(false);
    }
  }
  async function send() {
    const text = draft.trim();
    if (!connected || !connection.current || !text || sending.current) return;
    sending.current = true;
    setBusy(true);
    if (pending.current?.text !== text)
      pending.current = { text, id: api.newId() };
    try {
      await connection.current.send(pending.current.id, text);
      setDraft((current) => (current === draft ? '' : current));
      pending.current = null;
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Sending failed.');
    } finally {
      sending.current = false;
      setBusy(false);
    }
  }
  return {
    pair,
    connect,
    connected,
    snapshot,
    shell,
    busy,
    error,
    retry,
    send,
    draft,
    setDraft,
  };
}
