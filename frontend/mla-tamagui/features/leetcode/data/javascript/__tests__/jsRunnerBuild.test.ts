import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { afterEach, describe, expect, it } from 'vitest';

import { JS_PAGE_HTML } from '../jsPageHtml';

type Build = { buildPrelude(): Promise<string>; buildPage(options: { prelude: string; entry?: string }): Promise<string> };

const SCRIPT = resolve(fileURLToPath(new URL('.', import.meta.url)), '../../../../../scripts/js-runner-build.mjs');
const build = (): Promise<Build> => import(/* @vite-ignore */ pathToFileURL(SCRIPT).href) as Promise<Build>;

const dirs: string[] = [];
function pageSource(text: string): string {
  const dir = mkdtempSync(join(tmpdir(), 'js-runner-page-'));
  dirs.push(dir);
  const file = join(dir, 'page.ts');
  writeFileSync(file, text);
  return file;
}

describe('the build of the JavaScript runner’s page', () => {
  afterEach(() => {
    for (const dir of dirs.splice(0)) rmSync(dir, { recursive: true, force: true });
  });

  it('Given the worker harness, When the prelude is built, Then it is one line of ASCII, so it cannot shift a line of the learner’s code', async () => {
    const prelude = await (await build()).buildPrelude();
    expect(prelude.length).toBeGreaterThan(1000);
    expect(prelude).not.toMatch(/[\n\r\u2028\u2029]/);
    expect(prelude).toMatch(/^[\x20-\x7e]*$/);
  });

  it('Given a page that reads the prelude constant, When it is built, Then the prelude is in the page as a string and no constant is left to read', async () => {
    const { buildPage } = await build();
    const js = await buildPage({ prelude: 'var harness = 1', entry: pageSource('window.carried = __JS_RUNNER_PRELUDE__;\n') });
    expect(js).toContain('"var harness = 1"');
    expect(js).not.toContain('__JS_RUNNER_PRELUDE__');
  });

  it('Given a page whose source holds a closing script tag in a string, When it is built, Then none is left raw, so the html around it cannot end the script early', async () => {
    const { buildPage } = await build();
    const js = await buildPage({ prelude: '', entry: pageSource("window.carried = '</SCRIPT>';\n") });
    expect(js).not.toMatch(/<\/script/i);
    expect(js).toContain('<\\/SCRIPT>');
  });

  it('Given the generated page, When it is wrapped as html, Then the document holds the page as its one script and that script is never closed early', () => {
    expect(JS_PAGE_HTML.startsWith('<!doctype html>')).toBe(true);
    expect(JS_PAGE_HTML.split('<script>')).toHaveLength(2);
    expect(JS_PAGE_HTML.split('</script>')).toHaveLength(2);
    expect(JS_PAGE_HTML.endsWith('</script></body></html>')).toBe(true);
  });
});
