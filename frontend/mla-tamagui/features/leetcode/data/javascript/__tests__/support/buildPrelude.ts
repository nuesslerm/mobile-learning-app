import { resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

let built: Promise<string> | undefined;

/** The prelude the page ships, built by the same script as the page, once per test file. */
export function shippedPrelude(): Promise<string> {
  built ??= (async () => {
    const script = resolve(fileURLToPath(new URL('.', import.meta.url)), '../../../../../../scripts/js-runner-build.mjs');
    const module = (await import(/* @vite-ignore */ pathToFileURL(script).href)) as { buildPrelude(): Promise<string> };
    return module.buildPrelude();
  })();
  return built;
}
