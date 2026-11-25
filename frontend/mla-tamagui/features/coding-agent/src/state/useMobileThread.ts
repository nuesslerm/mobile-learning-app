import { useEffect, useRef, useState } from 'react';
import { ThreadNotFound } from '../domain/mobile';
import type { AgentEvent, MobilePort, Pairing, ThreadContext, TranscriptItem } from '../domain/mobile';

export function useMobileThread(api: MobilePort, thread: string) {
  const [pair, setPair] = useState<Pairing | null>();
  const [context, setContext] = useState<ThreadContext>();
  const [events, setEvents] = useState<AgentEvent[]>([]);
  const [status, setStatus] = useState('Connecting…');
  const [error, setError] = useState('');
  const [legacy, setLegacy] = useState(false);
  const [childId, setChildId] = useState<string>();
  const [transcript, setTranscript] = useState<{ rows: TranscriptItem[]; message: string }>();
  useEffect(() => {
    let active = true;
    setTranscript(undefined);
    if (childId && pair) {
      api.transcript(pair, childId).then(items => {
        const rows = items.filter(item => item.kind === 'message' && item.text);
        if (active) setTranscript({ rows, message: rows.length ? '' : 'No transcript messages yet.' });
      }).catch(e => {
        if (active) setTranscript({ rows: [], message: e instanceof ThreadNotFound
          ? 'RCA has activity for this agent. A separate transcript is unavailable.' : String(e) });
      });
    }
    return () => { active = false; };
  }, [api, childId, pair]);
  const [draft, setDraft] = useState('');
  const attempt = useRef<{ id: string; prompt: string } | undefined>(undefined);
  const sending = useRef(false);
  useEffect(() => {
    let active = true;
    let cursor = 0;
    let failures = 0;
    let timer: ReturnType<typeof setTimeout>;
    let stop: (() => void) | undefined;
    setChildId(undefined); setLegacy(false); setEvents([]); setContext(undefined); setPair(undefined); setError(''); setStatus('Connecting…');
    setDraft(''); attempt.current = undefined;
    async function connect() {
      try {
        const saved = await api.readPairing();
        if (!active) return;
        setPair(saved);
        if (!saved) return;
        const snapshot = await api.context(saved, thread);
        if (!active) return;
        setContext(snapshot);
        for (;;) {
          const page = await api.history(saved, thread, cursor);
          if (!active) return;
          const fresh = page.events.filter(event => event.seq > cursor);
          if (fresh.length) {
            cursor = fresh[fresh.length - 1].seq;
            setEvents(old => [...old, ...fresh]);
          }
          if (!page.more) break;
          if (!fresh.length) throw new Error('RCA history cursor did not advance');
        }
        setStatus(''); setError('');
        stop = api.follow(saved, thread, cursor, event => {
          if (!active || event.seq <= cursor) return;
          failures = 0;
          cursor = event.seq;
          setEvents(old => [...old, event]);
          if (event.type === 'turn.finished') {
            api.context(saved, thread).then(next => { if (active) setContext(next); })
              .catch(e => { if (active) setError(String(e)); });
          }
        }, reconnect);
      } catch (e) {
        if (!active) return;
        if (e instanceof ThreadNotFound) setLegacy(true);
        else { setError(String(e)); reconnect(); }
      }
    }
    function reconnect() {
      if (!active) return;
      setStatus('Reconnecting…');
      timer = setTimeout(() => { void connect(); }, Math.min(1000 * 2 ** failures++, 10000));
    }
    void connect();
    return () => { active = false; clearTimeout(timer); stop?.(); };
  }, [api, thread]);

  async function send() {
    if (!pair || sending.current || !draft.trim()) return;
    sending.current = true;
    const prompt = draft.trim();
    if (attempt.current?.prompt !== prompt) attempt.current = { id: api.newId(), prompt };
    try {
      await api.send(pair, thread, attempt.current.id, prompt);
      setDraft(''); attempt.current = undefined; setError('');
    } catch (e) { setError(String(e)); }
    finally { sending.current = false; }
  }
  async function respond(id: string, kind: 'approvals' | 'questions', value: string) {
    if (!pair) return;
    try { await api.respond(pair, thread, id, kind, value); setError(''); }
    catch (e) { setError(String(e)); }
  }
  return { childId, setChildId, transcript, legacy, pair, context, events, status, error, draft, setDraft, send, respond };
}
