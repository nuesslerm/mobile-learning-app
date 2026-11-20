import { Logger } from '@lib/logging';
import { createRpcClient, createWebViewHostTransport, type RpcClient } from '@lib/webview-rpc';
import { Asset } from 'expo-asset';
import { File, FileMode } from 'expo-file-system';
import { useEffect, useRef, useState } from 'react';
import { ScrollView, Text, View } from 'react-native';
import { WebView } from 'react-native-webview';

type Names = 'asm' | 'lock' | 'wasm' | 'stdlib';
export type PyodideSpikeAssets = Record<Names, number>;

// where the WebView loads the page from: a file beside the assets on a phone (so file access can be granted to that directory), html on the web
type PageSource = { uri: string; directory: string } | { html: string };

const CHUNK_BYTES = 393216;
const READY_TIMEOUT_MS = 90_000;

function base64Of(bytes: Uint8Array): string {
  let binary = '';
  for (let i = 0; i < bytes.length; i += 8192) binary += String.fromCharCode(...bytes.subarray(i, i + 8192));
  return btoa(binary);
}

async function pushChunks(client: RpcClient, name: Names, uri: string): Promise<void> {
  const handle = new File(uri).open(FileMode.ReadOnly);
  try {
    const size = handle.size ?? 0;
    const total = Math.max(1, Math.ceil(size / CHUNK_BYTES));
    for (let index = 0; index < total; index += 1) {
      const bytes = await handle.readBytes(CHUNK_BYTES);
      await client.call('chunk', { name, index, total, data: base64Of(bytes) }, { timeoutMs: 60_000 });
    }
  } finally {
    handle.close();
  }
}

async function roundTrip(client: RpcClient, calls: number): Promise<number> {
  const started = Date.now();
  for (let i = 0; i < calls; i += 1) await client.call('ping');
  return Math.round(((Date.now() - started) / calls) * 10) / 10;
}

function pageSourceOf(wasmUri: string, pageHtml: string): PageSource {
  if (!wasmUri.startsWith('file:')) return { html: pageHtml };
  const directory = wasmUri.slice(0, wasmUri.lastIndexOf('/') + 1);
  const file = new File(`${directory}pyodide-page.html`);
  if (!file.exists) file.create();
  file.writeSync(pageHtml);
  return { uri: file.uri, directory };
}

/**
 * A throwaway probe of the A5 spike: boot the bundled Pyodide in a hidden WebView over the shared bridge, run two
 * programs, and print the numbers. Deleted when the Pyodide runner adapter lands.
 */
export function PyodideSpikeScreen({ assets, pageHtml }: { assets: PyodideSpikeAssets; pageHtml: string }) {
  const webRef = useRef<WebView>(null);
  const [transport] = useState(() => createWebViewHostTransport(() => webRef.current));
  const [client] = useState(() => createRpcClient(transport, { timeoutMs: 240_000 }));
  const [steps, setSteps] = useState<string[]>([]);
  const [result, setResult] = useState<string>('');
  // undefined until the assets are on disk: the WebView mounts after, so its page loads once
  const [page, setPage] = useState<PageSource | undefined>(undefined);

  useEffect(() => {
    let cancelled = false;
    const started = Date.now();
    const marks: Record<string, number> = {};
    const mark = (name: string, fields: Record<string, unknown> = {}) => {
      marks[name] = Date.now() - started;
      Logger.info('pyodide_spike.step', { step: name, atMs: marks[name], ...fields });
      if (!cancelled) setSteps((previous) => [...previous, `${name} ${marks[name]}ms`]);
    };
    const failure = (error: unknown) => {
      const message = error instanceof Error ? error.message : String(error);
      Logger.warn('pyodide_spike.failed', { message });
      if (!cancelled) setResult(JSON.stringify({ error: message, marks }));
    };

    // subscribed before the WebView exists, so the page's one `ready` event cannot be missed
    const ready = new Promise<void>((resolve, reject) => {
      const stop = client.subscribe('ready', () => {
        stop();
        resolve();
      });
      setTimeout(() => reject(new Error('the Pyodide page never said it was ready')), READY_TIMEOUT_MS);
    });

    (async () => {
      const names = Object.keys(assets) as Names[];
      const loaded = await Promise.all(names.map((name) => Asset.fromModule(assets[name]).downloadAsync()));
      const uris = Object.fromEntries(names.map((name, i) => [name, loaded[i]?.localUri ?? loaded[i]?.uri ?? ''])) as Record<Names, string>;
      mark('assetsResolved');
      const source = pageSourceOf(uris.wasm, pageHtml);
      const mounted = Date.now();
      setPage(source);
      await ready;
      mark('pageReady', { pageMs: Date.now() - mounted, pageFromFile: 'uri' in source });
      const roundTripMs = await roundTrip(client, 20);
      let strategy = 'pull';
      let pullError: string | null = null;
      const transferStarted = Date.now();
      try {
        await client.call('pull', uris, { timeoutMs: 120_000 });
      } catch (error) {
        strategy = 'chunks';
        pullError = error instanceof Error ? error.message : String(error);
        Logger.warn('pyodide_spike.pull_failed', { message: pullError });
        for (const name of names) await pushChunks(client, name, uris[name]);
      }
      const transferMs = Date.now() - transferStarted;
      mark('filesInPage', { strategy, transferMs });
      const boot = await client.call<{ importMs: number; loadMs: number; version: string }>('boot', undefined, { timeoutMs: 240_000 });
      mark('booted', boot);
      const first = await client.call<{ stdout: string; value: string | null; error: string | null; ms: number }>('run', { code: 'print(1 + 1)' });
      mark('ran');
      const warm = await client.call<{ ms: number; value: string | null }>('run', { code: 'sum(i * i for i in range(1_000_000))' });
      const memory = await client.call('memory');
      if (!cancelled) setResult(JSON.stringify({ strategy, pullError, transferMs, roundTripMs, boot, first, warm, memory, marks }));
    })().catch(failure);

    return () => {
      cancelled = true;
      client.close();
    };
  }, [assets, client, pageHtml]);

  return (
    <View style={{ flex: 1, padding: 16 }} testID="PyodideSpikeScreen">
      <ScrollView>
        <Text testID="PyodideSpikeSteps">{steps.join('\n')}</Text>
        <Text selectable testID="PyodideSpikeResult">
          {result}
        </Text>
      </ScrollView>
      {page === undefined ? null : (
        <View pointerEvents="none" style={{ position: 'absolute', width: 1, height: 1, opacity: 0 }}>
          <WebView<object>
            allowFileAccess
            allowFileAccessFromFileURLs
            allowUniversalAccessFromFileURLs
            allowingReadAccessToURL={'directory' in page ? page.directory : undefined}
            javaScriptEnabled
            mixedContentMode="always"
            onMessage={(event) => transport.receive(event.nativeEvent.data)}
            originWhitelist={['*']}
            ref={webRef}
            source={'uri' in page ? { uri: page.uri } : { html: page.html }}
            testID="PyodideSpikeWebView"
          />
        </View>
      )}
    </View>
  );
}
