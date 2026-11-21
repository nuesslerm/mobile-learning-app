import type { RpcTransport } from '../ports/transport';

const later = (fn: () => void) => void Promise.resolve().then(fn);

/** Two transports wired to each other in memory, delivering a tick later as a real channel does. */
export function createLoopbackPair(): { host: RpcTransport; page: RpcTransport } {
  const toHost = new Set<(raw: string) => void>();
  const toPage = new Set<(raw: string) => void>();
  const side = (inbox: Set<(raw: string) => void>, outbox: Set<(raw: string) => void>): RpcTransport => ({
    send: (raw) => later(() => [...outbox].forEach((listener) => listener(raw))),
    listen(onRaw) {
      inbox.add(onRaw);
      return () => {
        inbox.delete(onRaw);
      };
    },
  });
  return { host: side(toHost, toPage), page: side(toPage, toHost) };
}
