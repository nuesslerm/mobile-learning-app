// The page side of the WebView rpc test: the real library's server and page transport, bundled into the iframe.
import { RpcError, createPageTransport, createRpcServer } from '../../../../libraries/webview-rpc/src';

const page = window as unknown as Parameters<typeof createPageTransport>[0];

const server = createRpcServer(createPageTransport(page), {
  add: (p) => (p as { a: number; b: number }).a + (p as { a: number; b: number }).b,
  boom: () => {
    throw new RpcError('remote', 'it broke', { where: 'page' });
  },
  hang: () => new Promise(() => undefined),
  echo: (p) => new Promise((resolve) => setTimeout(() => resolve(p), (p as { ms: number }).ms)),
  startTicks: (p) => {
    for (let n = 1; n <= (p as { count: number }).count; n += 1) server.emit('tick', n);
    return 'sent';
  },
});

server.emit('ready');
