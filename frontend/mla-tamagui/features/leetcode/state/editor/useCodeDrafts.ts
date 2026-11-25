import { Logger } from '@lib/logging';
import { useCallback, useEffect, useRef, useState } from 'react';

import { useLeetCodePorts } from '../ports';

export type DraftStatus = 'saved' | 'saving' | 'unsaved';

export type CodeDraftSession = {
  read: (language: string) => string | undefined;
  save: (language: string, code: string | null) => void;
  status: (language: string) => DraftStatus;
};

export function useCodeDrafts(problemId: string | undefined, languages: readonly string[]) {
  const { codeDrafts } = useLeetCodePorts();
  const values = useRef(new Map<string, string>());
  const writes = useRef(new Map<string, Promise<void>>());
  const [load, setLoad] = useState<{ id: string; ok: boolean }>();
  const [statuses, setStatuses] = useState<Partial<Record<string, DraftStatus>>>({});
  const ready = problemId !== undefined && load?.id === problemId && load.ok;

  useEffect(() => {
    if (problemId === undefined) return;
    let current = true;
    void Promise.all(languages.map(async (language) => [language, await codeDrafts.read(problemId, language)] as const)).then(
      (entries) => {
        if (!current) return;
        values.current = new Map(entries.flatMap(([language, code]) => code === null ? [] : [[language, code]]));
        writes.current.clear();
        setStatuses({});
        setLoad({ id: problemId, ok: true });
      },
      (error: unknown) => {
        if (!current) return;
        Logger.withError(error).error('leetcode.draft.read_failed');
        setLoad({ id: problemId, ok: false });
      },
    );
    return () => { current = false; };
  }, [codeDrafts, languages, problemId]);

  const read = useCallback((language: string) => values.current.get(language), []);
  const save = useCallback((language: string, code: string | null) => {
    if (problemId === undefined || !ready) return;
    if (code === null) values.current.delete(language);
    else values.current.set(language, code);
    setStatuses((previous) => ({ ...previous, [language]: 'saving' }));
    const write = codeDrafts.write(problemId, language, code);
    writes.current.set(language, write);
    const settle = (status: DraftStatus) => {
      if (writes.current.get(language) !== write) return;
      writes.current.delete(language);
      setStatuses((previous) => ({ ...previous, [language]: status }));
    };
    void write.then(
      () => settle('saved'),
      (error: unknown) => {
        Logger.withError(error).error('leetcode.draft.write_failed');
        settle('unsaved');
      },
    );
  }, [codeDrafts, problemId, ready]);

  const session: CodeDraftSession | undefined = ready
    ? { read, save, status: (language) => statuses[language] ?? 'saved' }
    : undefined;
  return { session, loading: problemId !== undefined && load?.id !== problemId };
}
