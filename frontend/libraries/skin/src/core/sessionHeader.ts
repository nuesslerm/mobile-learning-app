import { useCallback, useEffect, useRef, useState, type ComponentRef } from 'react';
import type { ScrollView } from 'react-native';

import type { SessionHeaderProps } from '../ports/ChatComponents';

export type SessionFileSource = NonNullable<SessionHeaderProps['files']>;
type Listing = Awaited<ReturnType<SessionFileSource['listDirectory']>>;
export type SessionFileEntry = Listing['entries'][number];
type FolderState = { result?: Listing; failed?: boolean };

export type FileTreeRow =
  | { entry: SessionFileEntry; depth: number }
  | { status: string; parent: string; depth: number; retry?: boolean };

const HIDDEN = new Set(['.git', 'node_modules']);

export function fileName(path: string): string {
  return path.split('/').pop() ?? path;
}

/** A workspace's files as rows, whatever they look like: folders load when first opened, and stay open and loaded for as long as the tree is mounted. */
export function useFileTree(source: SessionFileSource) {
  const [folders, setFolders] = useState<Record<string, FolderState>>({});
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const generation = useRef(0);
  const requested = useRef(new Set<string>());
  const load = useCallback(
    (path: string) => {
      if (requested.current.has(path)) return;
      requested.current.add(path);
      const active = generation.current;
      setFolders((prev) => ({ ...prev, [path]: {} }));
      void source.listDirectory(path).then(
        (result) => {
          if (active === generation.current) setFolders((prev) => ({ ...prev, [path]: { result } }));
        },
        () => {
          if (active !== generation.current) return;
          requested.current.delete(path);
          setFolders((prev) => ({ ...prev, [path]: { failed: true } }));
        }
      );
    },
    [source]
  );
  useEffect(() => {
    requested.current.clear();
    setExpanded(new Set());
    setFolders({});
    load('');
    return () => {
      generation.current++;
    };
  }, [load]);

  const rows: FileTreeRow[] = [];
  function append(path: string, depth: number) {
    const folder = folders[path];
    if (!folder?.result) {
      rows.push({
        status: folder?.failed ? 'Could not load files. Try again' : 'Loading files…',
        parent: path,
        depth,
        retry: folder?.failed,
      });
      return;
    }
    const entries = folder.result.entries
      .filter((entry) => !HIDDEN.has(fileName(entry.path)))
      .sort((a, b) =>
        a.kind === b.kind
          ? a.path.localeCompare(b.path, undefined, { numeric: true, sensitivity: 'base' })
          : a.kind === 'directory'
            ? -1
            : 1
      );
    for (const entry of entries) {
      rows.push({ entry, depth });
      if (entry.kind === 'directory' && expanded.has(entry.path)) append(entry.path, depth + 1);
    }
    if (folder.result.truncated || entries.length === 0)
      rows.push({ status: folder.result.truncated ? 'Listing truncated' : 'No files found', parent: path, depth });
  }
  append('', 0);

  function toggle(path: string) {
    const next = new Set(expanded);
    if (next.has(path)) next.delete(path);
    else {
      next.add(path);
      load(path);
    }
    setExpanded(next);
  }
  return {
    loading: !folders['']?.result && !folders['']?.failed,
    rows,
    expanded,
    load,
    toggle,
  };
}

/** One file's contents as they load; a failed read can be tried again. */
export function useFilePreview(source: SessionFileSource, path: string) {
  const [file, setFile] = useState<Awaited<ReturnType<SessionFileSource['readFile']>>>();
  const [failed, setFailed] = useState(false);
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let active = true;
    setFile(undefined);
    setFailed(false);
    void source.readFile(path).then(
      (value) => {
        if (active) setFile(value);
      },
      () => {
        if (active) setFailed(true);
      }
    );
    return () => {
      active = false;
    };
  }, [source, path, attempt]);
  return { file, failed, retry: () => setAttempt((value) => value + 1) };
}

export type SessionMenuPage = 'menu' | 'files' | 'text';

/** Where the header's "more" menu is: closed, its list, the file explorer (with the file that is open in it) or the chat text. */
export function useSessionMenu() {
  const [page, setPage] = useState<SessionMenuPage>();
  const [path, setPath] = useState('');
  return {
    page,
    setPage,
    path,
    setPath,
    back: () => {
      if (path) setPath('');
      else setPage(undefined);
    },
    close: () => {
      setPath('');
      setPage(undefined);
    },
    backLabel: path ? 'Back to files' : 'Back to chat',
  };
}

/** Keeps the picked tab in sight in a strip that scrolls sideways. */
export function useTabStrip(activeId: string | undefined) {
  const strip = useRef<ComponentRef<typeof ScrollView>>(null);
  const positions = useRef(new Map<string, number>());
  const reveal = () => {
    const x = activeId ? positions.current.get(activeId) : undefined;
    if (x !== undefined) strip.current?.scrollTo({ x: Math.max(0, x - 12), animated: false });
  };
  useEffect(reveal, [activeId]);
  return {
    strip,
    reveal,
    place: (id: string, x: number) => {
      positions.current.set(id, x);
      if (id === activeId) reveal();
    },
  };
}

export function sessionStatus(connected: boolean, branch: string | null | undefined): string {
  return connected ? (branch ?? 'Connected') : 'Connecting…';
}

export function tabCountLabel(count: number): string {
  return count === 1 ? '1 tab' : `${count} tabs`;
}
