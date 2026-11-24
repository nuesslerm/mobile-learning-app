import { describe, expect, it } from 'vitest';

import { resolvePage, type AssetIo } from '../../pyodide/pyodideAssets';
import { createWebViewPageHost } from '../webViewPageHost';

const MODULES = { asm: 1, lock: 2, wasm: 3, stdlib: 4 };

function setup(resolve: AssetIo['resolve'] = async (id) => `http://x/${id}.bin`) {
  let resolved = 0;
  const io: AssetIo = {
    resolve: async (id) => {
      resolved += 1;
      return resolve(id);
    },
    async writePage() {
      return 'file:///p.html';
    },
    chunks: async function* () {},
  };
  const created = createWebViewPageHost({ resolve: () => resolvePage(MODULES, '<html>', io), chunks: io.chunks });
  const scripts: string[] = [];
  created.view.bind({ injectJavaScript: (script) => void scripts.push(script) });
  return { ...created, scripts, resolutions: () => resolved };
}

describe('the WebView page host', () => {
  it('Given a prepared session, When it is shown and closed, Then the view mounts the page of that generation and then removes it', async () => {
    const { host, view } = setup();
    expect(view.snapshot()).toBeNull();
    const session = await host.prepare();
    session.show();
    expect(view.snapshot()).toMatchObject({ generation: 1, source: { html: '<html>' } });
    host.close();
    expect(view.snapshot()).toBeNull();
  });

  it('Given two pages, When the older one sends a message late, Then the new page’s listeners never hear it', async () => {
    const { host, view } = setup();
    const first = await host.prepare();
    first.show();
    const second = await host.prepare();
    second.show();
    const heard: string[] = [];
    second.transport.listen((raw) => heard.push(raw));
    view.receive(1, 'from the old page');
    view.receive(2, 'from the new page');
    expect(heard).toEqual(['from the new page']);
  });

  it('Given two pages, When the old one is reported lost, Then only the new session’s listener hears about its own page', async () => {
    const { host, view } = setup();
    const first = await host.prepare();
    const second = await host.prepare();
    const lost: string[] = [];
    second.onLost((reason) => lost.push(reason));
    view.lost(1, 'old page died');
    view.lost(2, 'new page died');
    expect(lost).toEqual(['new page died']);
    void first;
  });

  it('Given a session of an older generation, When it sends, Then nothing is injected into the page that replaced it', async () => {
    const { host, scripts } = setup();
    const first = await host.prepare();
    await host.prepare();
    expect(() => first.transport.send('late')).toThrow(/not mounted/);
    expect(scripts).toEqual([]);
  });

  it('Given the current session, When it sends, Then the message is injected into the bound page', async () => {
    const { host, scripts } = setup();
    const session = await host.prepare();
    session.transport.send('hello');
    expect(scripts).toHaveLength(1);
    expect(scripts[0]).toContain('hello');
  });

  it('Given several pages, When each is prepared, Then the assets were resolved once', async () => {
    const { host, resolutions } = setup();
    await host.prepare();
    await host.prepare();
    expect(resolutions()).toBe(4);
  });

  it('Given an asset that failed to resolve, When the next page is prepared, Then the failure was not remembered', async () => {
    let fail = true;
    const { host } = setup(async (id) => (fail ? Promise.reject(new Error('offline')) : `http://x/${id}.bin`));
    await expect(host.prepare()).rejects.toMatchObject({ stage: 'assets' });
    fail = false;
    await expect(host.prepare()).resolves.toBeDefined();
  });
});
