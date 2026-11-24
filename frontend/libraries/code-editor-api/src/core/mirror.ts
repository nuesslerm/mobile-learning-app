import type { EditorEngine } from '../ports/engine';
import type { Change, EditorEvent, Range, Revision, Selection } from '../ports/protocol';
import { applyChanges, diffChange, mapRange } from './changes';
import { createEditorClient, unwrap } from './client';

export type TrackedRange = {
  /** null once the editor instance it described is gone */
  get(): Range | null;
  /** an edit changed text strictly inside it, or overlapped it */
  touched(): boolean;
  /** an edit replaced a span that covered it */
  deleted(): boolean;
  release(): void;
};

export type DocumentMirror = {
  text(): string;
  revision(): Revision;
  selection(): Selection;
  instance(): string;
  track(range: Range): TrackedRange;
  onChange(listener: () => void): () => void;
  resync(): Promise<void>;
  stop(): void;
};

type Tracked = { range: Range | null; touched: boolean; deleted: boolean };

function follow(tracked: Tracked, changes: readonly Change[]): void {
  const range = tracked.range;
  if (!range) return;
  for (const change of changes) {
    const replaces = change.to > change.from;
    if (replaces && change.from <= range.from && change.to >= range.to) tracked.deleted = true;
    const overlaps = replaces && change.from < range.to && change.to > range.from;
    const inside = !replaces && change.from > range.from && change.from < range.to;
    if (overlaps || inside) tracked.touched = true;
  }
  tracked.range = mapRange(range, changes);
}

/** A local copy of the document kept in step by `changed` events: events are subscribed and buffered before the first read, so none is lost. */
export async function createDocumentMirror(engine: EditorEngine, onError?: (error: unknown) => void): Promise<DocumentMirror> {
  const client = createEditorClient(engine);
  const tracked = new Set<Tracked>();
  const listeners = new Set<() => void>();
  let text = '';
  let revision = 0;
  let selection: Selection = { ranges: [{ anchor: 0, head: 0 }], main: 0 };
  let instance = '';
  let buffer: EditorEvent[] | null = [];
  let syncing: Promise<void> | null = null;

  const resyncing = () => void resync().catch((error: unknown) => onError?.(error));
  const notify = () => {
    for (const listener of [...listeners]) listener();
  };
  const resync = (): Promise<void> => {
    syncing ??= (async () => {
      try {
        const hello = unwrap(await client.hello());
        const reply = await client.getText();
        const read = unwrap(reply);
        const previous = text;
        if (hello.instance !== instance && instance !== '') for (const item of tracked) item.range = null;
        else {
          const diff = diffChange(previous, read.text);
          if (diff) for (const item of tracked) follow(item, [diff]);
        }
        instance = hello.instance;
        text = read.text;
        revision = reply.ok ? reply.revision : revision;
        selection = read.selection;
        notify();
      } finally {
        syncing = null;
      }
    })();
    return syncing;
  };
  const apply = (event: EditorEvent) => {
    if (event.type === 'ready') {
      if (event.instance !== instance) resyncing();
      return;
    }
    if (event.type === 'selection') {
      selection = event.selection;
      notify();
      return;
    }
    if (event.type !== 'changed') return;
    if (event.from !== revision) {
      resyncing();
      return;
    }
    text = applyChanges(text, event.changes);
    revision = event.to;
    selection = event.selection;
    for (const item of tracked) follow(item, event.changes);
    notify();
  };

  const stopListening = engine.listen(['ready', 'changed', 'selection'], (event) => {
    if (buffer) buffer.push(event);
    else apply(event);
  });
  await resync();
  const early = buffer ?? [];
  buffer = null;
  for (const event of early) if (event.type !== 'changed' || event.to > revision) apply(event);

  return {
    text: () => text,
    revision: () => revision,
    selection: () => selection,
    instance: () => instance,
    track(range) {
      const item: Tracked = { range, touched: false, deleted: false };
      tracked.add(item);
      return { get: () => item.range, touched: () => item.touched, deleted: () => item.deleted, release: () => void tracked.delete(item) };
    },
    onChange(listener) {
      listeners.add(listener);
      return () => void listeners.delete(listener);
    },
    resync,
    stop: stopListening,
  };
}
