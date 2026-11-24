import { describe, expect, it } from 'vitest';

import { commonDirectory, resolvePage, type AssetIo } from '../pyodideAssets';

const MODULES = { asm: 1, lock: 2, wasm: 3, stdlib: 4 };

function io(over: Partial<AssetIo> & { uris?: Record<number, string> } = {}): AssetIo & { written: string[] } {
  const written: string[] = [];
  return {
    written,
    resolve: over.resolve ?? (async (id) => over.uris?.[id] ?? `file:///data/user/0/app/updates/${id}.bin`),
    writePage: over.writePage ?? (async (html) => {
      written.push(html);
      return 'file:///data/user/0/app/cache/pyodide-page.html';
    }),
    chunks: over.chunks ?? (async function* () {}),
  };
}

describe('commonDirectory', () => {
  it.each([
    [['file:///a/b/c.html', 'file:///a/b/d/e.bin'], 'file:///a/b/'],
    [['file:///a/b/c.html', 'file:///a/x/d.bin'], 'file:///a/'],
    [['file:///a/b/c.html', 'file:///a/b/c.html'], 'file:///a/b/'],
    [['file:///a/bc.html', 'file:///a/bd.bin'], 'file:///a/'],
    [['x'], ''],
  ])('Given %j, When the common directory is taken, Then it is %s', (uris, expected) => {
    expect(commonDirectory(...uris)).toBe(expected);
  });
});

describe('resolvePage', () => {
  it('Given file: assets, When the page is resolved, Then the page is written to the cache and loaded by uri with read access to the directory it shares with the assets', async () => {
    const fake = io();
    const { source, files } = await resolvePage(MODULES, '<html>', fake);
    expect(fake.written).toEqual(['<html>']);
    expect(source).toEqual({ uri: 'file:///data/user/0/app/cache/pyodide-page.html', directory: 'file:///data/user/0/app/' });
    expect(files.wasm).toBe('file:///data/user/0/app/updates/3.bin');
  });

  it('Given http assets, When the page is resolved, Then it travels as html and nothing is written', async () => {
    const fake = io({ resolve: async (id) => `http://localhost:8800/assets/${id}.bin` });
    const { source } = await resolvePage(MODULES, '<html>', fake);
    expect(source).toEqual({ html: '<html>' });
    expect(fake.written).toEqual([]);
  });

  it('Given an asset that cannot be resolved, When the page is resolved, Then it fails as unavailable at the assets stage', async () => {
    await expect(resolvePage(MODULES, '<html>', io({ resolve: async () => Promise.reject(new Error('offline')) }))).rejects.toMatchObject({ kind: 'unavailable', stage: 'assets', message: 'offline' });
  });

  it('Given a cache that cannot be written, When the page is resolved, Then it fails as unavailable at the page stage', async () => {
    await expect(resolvePage(MODULES, '<html>', io({ writePage: async () => Promise.reject(new Error('read only')) }))).rejects.toMatchObject({ kind: 'unavailable', stage: 'page' });
  });
});
