import { Logger } from '@lib/logging';
import { createRpcClient, createWebViewHostTransport, type RpcClient } from '@lib/webview-rpc';
import { Asset } from 'expo-asset';
import { File, FileMode } from 'expo-file-system';
import { useEffect, useRef, useState } from 'react';
import { ScrollView, Text, View } from 'react-native';
import { WebView } from 'react-native-webview';

type Names = 'asm' | 'lock' | 'wasm' | 'stdlib';
export type PyodideSpikeAssets = Record<Names, number>;

const CHUNK_BYTES = 393216;

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

async function waitForPage(client: RpcClient): Promise<number> {
  const started = Date.now();
  while (Date.now() - started < 90_000) {
    try {
      await client.call('ping', undefined, { timeoutMs: 1000 });
      return Date.now() - started;
    } catch (error) {
      // the page has not loaded its script yet (or the WebView has not mounted): look again in a moment, never in a tight loop
      Logger.debug('pyodide_spike.page_not_ready', { message: error instanceof Error ? error.message : String(error) });
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
  }
  throw new Error('the Pyodide page never answered');
}

async function roundTrip(client: RpcClient, calls: number): Promise<number> {
  const started = Date.now();
  for (let i = 0; i < calls; i += 1) await client.call('ping');
  return Math.round(((Date.now() - started) / calls) * 10) / 10;
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
  // undefined until the assets are on disk, null when they are not files (the web): the WebView mounts after, so its page loads once
  const [directory, setDirectory] = useState<string | null | undefined>(undefined);

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

    (async () => {
      const names = Object.keys(assets) as Names[];
      const loaded = await Promise.all(names.map((name) => Asset.fromModule(assets[name]).downloadAsync()));
      const uris = Object.fromEntries(names.map((name, i) => [name, loaded[i]?.localUri ?? loaded[i]?.uri ?? ''])) as Record<Names, string>;
      mark('assetsResolved');
      const wasm = uris.wasm;
      setDirectory(wasm.startsWith('file:') ? wasm.slice(0, wasm.lastIndexOf('/') + 1) : null);
      const pageMs = await waitForPage(client);
      mark('pageReady', { pageMs });
      const roundTripMs = await roundTrip(client, 20);
      let strategy = 'pull';
      try {
        await client.call('pull', uris, { timeoutMs: 120_000 });
      } catch (error) {
        strategy = 'chunks';
        Logger.info('pyodide_spike.pull_failed', { message: error instanceof Error ? error.message : String(error) });
        for (const name of names) await pushChunks(client, name, uris[name]);
      }
      mark('filesInPage', { strategy });
      const boot = await client.call<{ importMs: number; loadMs: number; version: string }>('boot', undefined, { timeoutMs: 240_000 });
      mark('booted', boot);
      const first = await client.call<{ stdout: string; value: string | null; error: string | null; ms: number }>('run', { code: 'print(1 + 1)' });
      mark('ran');
      const warm = await client.call<{ ms: number; value: string | null }>('run', { code: 'sum(i * i for i in range(1_000_000))' });
      const memory = await client.call('memory');
      if (!cancelled) setResult(JSON.stringify({ strategy, roundTripMs, boot, first, warm, memory, marks }));
    })().catch(failure);

    return () => {
      cancelled = true;
      client.close();
    };
  }, [assets, client]);

  return (
    <View style={{ flex: 1, padding: 16 }} testID="PyodideSpikeScreen">
      <ScrollView>
        <Text testID="PyodideSpikeSteps">{steps.join('\n')}</Text>
        <Text selectable testID="PyodideSpikeResult">
          {result}
        </Text>
      </ScrollView>
      {directory === undefined ? null : (
        <View pointerEvents="none" style={{ position: 'absolute', width: 1, height: 1, opacity: 0 }}>
          <WebView<object>
            allowFileAccess
            allowFileAccessFromFileURLs
            allowUniversalAccessFromFileURLs
            allowingReadAccessToURL={directory ?? undefined}
            javaScriptEnabled
            mixedContentMode="always"
            onMessage={(event) => transport.receive(event.nativeEvent.data)}
            originWhitelist={['*']}
            ref={webRef}
            source={{ html: pageHtml, baseUrl: directory ?? 'file:///' }}
            testID="PyodideSpikeWebView"
          />
        </View>
      )}
    </View>
  );
}
