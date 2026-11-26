import type { CheckoutRow, HomeSummary, WorkspaceCatalog } from '../domain/workspaceCatalog';
import { Logger } from '@lib/logging';
import { useCallback, useEffect, useRef, useState } from 'react';
import { resumeCommand, type T3ComposerEntry } from '../domain/t3Mobile';
import type { Pairing } from '../domain/mobile';
import type {
  T3Conversation,
  T3MobilePort,
  T3Snapshot,
} from '../domain/t3Mobile';

export function useT3Mobile(api: T3MobilePort, hostOpen: boolean, threadId?: string) {
  const [pair, setPair] = useState<Pairing | null>();
  const [home, setHome] = useState<HomeSummary>();
  const [catalog, setCatalog] = useState<WorkspaceCatalog>();
  const [snapshot, setSnapshot] = useState<T3Snapshot>();
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [modelPickerOpen, setModelPickerOpen] = useState(false);
  const [connected, setConnected] = useState(false);
  const [imageSession, setImageSession] = useState<T3Conversation>();
  const connection = useRef<T3Conversation | null>(null);
  const [generation, setGeneration] = useState(0);
  const [draft, setDraft] = useState('');
  const [notice, setNotice] = useState('');
  const noticeTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const pickedSkill = useRef<string | undefined>(undefined);
  useEffect(() => () => clearTimeout(noticeTimer.current), []);
  useEffect(() => {
    if (!draft) pickedSkill.current = undefined;
  }, [draft]);

  function showResumeNotice() {
    clearTimeout(noticeTimer.current);
    setNotice(resumeCommand.description);
    noticeTimer.current = setTimeout(() => setNotice(''), 4000);
  }

  function changeDraft(next: string) {
    if (pickedSkill.current !== next.trimStart().split(/\s/, 1)[0]) pickedSkill.current = undefined;
    setDraft(next);
  }

  function pickSuggestion(row: T3ComposerEntry, leading: boolean) {
    if (row.kind === 'command' && row.token === resumeCommand.token) {
      showResumeNotice();
      return true;
    }
    if (leading) pickedSkill.current = row.kind === 'skill' ? row.token : undefined;
    return false;
  }
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
    const controller = new AbortController();
    setCatalog(undefined);
    setHome(undefined);
    setConnected(false);
    setImageSession(undefined);
    let current: T3Conversation | undefined;
    let timer: ReturnType<typeof setTimeout>;
    let failures = 0;
    function disconnected(reason: Error) {
      if (!active) return;
      setConnected(false);
      setImageSession(undefined);
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
          if (hostOpen) {
            const result = await api.workspaces(pair, controller.signal);
            if (active) setCatalog(result);
          } else {
            const result = await api.home(pair, controller.signal);
            if (active) setHome(result);
          }
          if (active) setError('');
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
        setImageSession(session);
      } catch (e) {
        Logger.warn('coding_agent.t3.connection_failed');
        disconnected(e instanceof Error ? e : new Error('Connection failed.'));
      }
    }
    void open();
    return () => {
      active = false;
      controller.abort();
      clearTimeout(timer);
      current?.close();
      connection.current = null;
    };
  }, [pair, api, threadId, hostOpen, generation]);

  async function connect(host: string, code: string, signal: AbortSignal) {
    const saved = await api.pair(host, code, signal);
    if (signal.aborted) return;
    setPair(saved);
    setError('');
  }
  async function send() {
    const text = draft.trim();
    if (!connected || !connection.current || !text || sending.current) return;
    const session = connection.current;
    const leading = text.split(/\s/, 1)[0];
    const isPickedSkill = leading === pickedSkill.current;
    if (!isPickedSkill && leading.toLowerCase() === resumeCommand.token &&
        session.commands?.some(row => row.token === resumeCommand.token)) {
      showResumeNotice();
      return;
    }
    clearTimeout(noticeTimer.current);
    setNotice('');
    sending.current = true;
    setBusy(true);
    if (pending.current?.text !== text)
      pending.current = { text, id: api.newId() };
    try {
      const modelCommand = /^\/model(?:\s+([^\r\n\u2028\u2029]+))?$/i.exec(draft.trimEnd());
      if (modelCommand && !isPickedSkill) {
        const thread = snapshot?.thread;
        const provider = thread?.modelSelection?.instanceId ?? thread?.providerInstanceId;
        if (!provider) throw new Error('The model picker is unavailable for this chat.');
        const argument = modelCommand[1]?.trim();
        if (argument) {
          const choices = await session.models(provider);
          if (connection.current !== session) return;
          const name = argument.toLowerCase();
          const choice = choices.find(row => row.name.toLowerCase() === name || row.slug.toLowerCase() === name);
          await session.setModel(pending.current.id, provider, choice?.slug ?? argument);
        } else {
          setModelPickerOpen(true);
        }
      } else {
        await session.send(pending.current.id, text);
      }
      if (connection.current !== session) return;
      setError('');
      setDraft((current) => (current === draft ? '' : current));
      pending.current = null;
    } catch (e) {
      if (connection.current === session) setError(e instanceof Error ? e.message : 'Sending failed.');
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
    imageSession: connected ? imageSession : undefined,
    catalog,
    home,
    updateCheckout(row: CheckoutRow) {
      setCatalog(current => current && {...current, checkouts:current.checkouts.map(r => r.id === row.id ? row : r)});
    },
    busy,
    error,
    retry,
    send,
    modelPickerOpen,
    setModelPickerOpen,
    draft,
    notice,
    pickSuggestion,
    changeDraft,
  };
}
