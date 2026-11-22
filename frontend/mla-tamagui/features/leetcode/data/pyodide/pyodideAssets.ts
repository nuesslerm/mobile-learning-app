import { CodeRunnerError } from '../../domain/ports';
import type { PageSource } from '../runner/pageHost';
import type { PyodideFileName } from './pageHandlers';

export type AssetIo = {
  /** a bundled asset's local uri (Asset.fromModule(id).downloadAsync()) */
  resolve(moduleId: number): Promise<string>;
  /** writes the page into the app's cache directory and returns its file: uri */
  writePage(html: string): Promise<string>;
  /** a file in 384 KiB base64 pieces: the fallback when the page cannot read it itself */
  chunks(
    uri: string
  ): AsyncIterable<{ index: number; total: number; data: string }>;
};

/** The longest directory every uri sits in: the WebView is granted read access to it, so the page can read assets that live elsewhere. */
export function commonDirectory(...uris: string[]): string {
  const [first = '', ...rest] = uris;
  let end = 0;
  for (
    let i = 0;
    i < first.length && rest.every((uri) => uri[i] === first[i]);
    i += 1
  )
    if (first[i] === '/') end = i + 1;
  return first.slice(0, end);
}

/**
 * The page and the urls of the runtime files. On a phone the assets are file: uris (Expo Go keeps an update's assets in its updates
 * directory, where the app may not write), so the page is written into the cache and loaded by uri with read access to the directory
 * both share. On the web the assets are http urls the page fetches, and the page travels as html.
 */
export async function resolvePage(
  modules: Record<PyodideFileName, number>,
  pageHtml: string,
  io: AssetIo
): Promise<{ source: PageSource; files: Record<PyodideFileName, string> }> {
  const names = Object.keys(modules) as PyodideFileName[];
  const uris = await Promise.all(
    names.map((name) => io.resolve(modules[name]))
  ).catch((error: unknown) => {
    throw new CodeRunnerError(
      'unavailable',
      error instanceof Error ? error.message : String(error),
      'assets'
    );
  });
  const files = Object.fromEntries(
    names.map((name, i) => [name, uris[i] ?? ''])
  ) as Record<PyodideFileName, string>;
  if (!files.wasm.startsWith('file:'))
    return { source: { html: pageHtml }, files };
  const uri = await io.writePage(pageHtml).catch((error: unknown) => {
    throw new CodeRunnerError(
      'unavailable',
      error instanceof Error ? error.message : String(error),
      'page'
    );
  });
  return {
    source: { uri, directory: commonDirectory(uri, ...Object.values(files)) },
    files,
  };
}
