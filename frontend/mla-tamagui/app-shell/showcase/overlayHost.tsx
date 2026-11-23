import {
  Fragment,
  useEffect,
  useSyncExternalStore,
  type ReactNode,
} from 'react';

// A bottom sheet fills its parent, so one rendered inside the scrolling page would hang in the middle of it. An item hands its
// sheets to this host, which the screen renders outside the ScrollView.
const entries = new Map<string, ReactNode>();
const listeners = new Set<() => void>();
let snapshot: [string, ReactNode][] = [];

function publish(): void {
  snapshot = [...entries];
  listeners.forEach((listener) => listener());
}

export function Overlay({ id, children }: { id: string; children: ReactNode }) {
  useEffect(() => {
    entries.set(id, children);
    publish();
  });
  useEffect(
    () => () => {
      entries.delete(id);
      publish();
    },
    [id]
  );
  return null;
}

export function OverlayHost() {
  const nodes = useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => snapshot,
    () => snapshot
  );
  return (
    <>
      {nodes.map(([id, node]) => (
        <Fragment key={id}>{node}</Fragment>
      ))}
    </>
  );
}
