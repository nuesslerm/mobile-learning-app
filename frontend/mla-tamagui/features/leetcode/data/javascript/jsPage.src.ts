// The page the hidden WebView runs for JavaScript: it answers the app's calls over @lib/webview-rpc and, from the next slice, starts
// the workers that run the learner's code. Compiled to jsPage.ts by scripts/js-runner-bundle.mjs.
import { createPageTransport, createRpcServer, type PageWindow } from '@lib/webview-rpc';

const server = createRpcServer(createPageTransport(window as unknown as PageWindow), { ping: () => Date.now() });
server.emit('ready');
