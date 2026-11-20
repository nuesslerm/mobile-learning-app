import { decode, type RpcTransport } from '@lib/webview-rpc';

export type FaultControls = {
  /** the next reply the page sends is lost */
  dropNextReply(): void;
  /** the next `changed` event the page emits is lost */
  swallowNextEvent(): void;
  /** the page stops hearing requests until the returned function runs */
  silentPage(): () => void;
  /** the host can no longer send */
  closeTransport(): void;
  /** the types of the events that crossed to the host so far */
  transportEvents(): readonly string[];
};

/** A transport pair that can lose a reply or an event, go quiet, or close: the failures a WebView channel has and a loopback does not. */
export function withFaults(pair: { host: RpcTransport; page: RpcTransport }): { host: RpcTransport; page: RpcTransport; controls: FaultControls } {
  let dropReply = false;
  let dropEvent = false;
  let silent = false;
  let closed = false;
  const crossed: string[] = [];
  const page: RpcTransport = {
    send(raw) {
      const decoded = decode(raw);
      if (decoded.type === 'message') {
        if (decoded.message.kind === 'res' && dropReply) {
          dropReply = false;
          return;
        }
        if (decoded.message.kind === 'evt') {
          const type = (decoded.message.data as { type?: string } | undefined)?.type;
          if (dropEvent && type === 'changed') {
            dropEvent = false;
            return;
          }
          if (typeof type === 'string' && decoded.message.topic === 'editor.event') crossed.push(type);
        }
      }
      pair.page.send(raw);
    },
    listen: (onRaw) =>
      pair.page.listen((raw) => {
        const decoded = decode(raw);
        if (silent && decoded.type === 'message' && decoded.message.kind === 'req') return;
        onRaw(raw);
      }),
  };
  const host: RpcTransport = {
    send(raw) {
      if (closed) throw new Error('the channel is closed');
      pair.host.send(raw);
    },
    listen: (onRaw) => pair.host.listen(onRaw),
  };
  return {
    host,
    page,
    controls: {
      dropNextReply: () => {
        dropReply = true;
      },
      swallowNextEvent: () => {
        dropEvent = true;
      },
      silentPage: () => {
        silent = true;
        return () => {
          silent = false;
        };
      },
      closeTransport: () => {
        closed = true;
      },
      transportEvents: () => crossed,
    },
  };
}
