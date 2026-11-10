import { describe, it, expect, vi } from 'vitest';

type NextLoad = (url: string, context: object) => Promise<unknown>;
type NodeLoaderModule = {
  load: (
    url: string,
    context: object,
    nextLoad: NextLoad
  ) => Promise<{ format: string; source: string; shortCircuit: boolean }>;
};

async function importNodeLoader(): Promise<NodeLoaderModule> {
  // @ts-expect-error -- .mjs has no type declarations; cast via NodeLoaderModule above.
  return import('../../src/babel/node-loader.mjs');
}

function wrapRequireWithoutPresetTypescript(req: NodeJS.Require) {
  return Object.assign(
    (specifier: string) => {
      if (specifier === '@babel/preset-typescript') {
        throw new Error(
          "Cannot find module '@babel/preset-typescript' (simulated: pruned devDependency in production)"
        );
      }
      return req(specifier);
    },
    {
      resolve: Object.assign(
        (specifier: string, options?: { paths?: string[] }) => {
          if (specifier === '@babel/preset-typescript') {
            throw new Error(
              "Cannot find module '@babel/preset-typescript' (simulated: pruned devDependency in production)"
            );
          }
          return req.resolve(specifier, options);
        },
        { paths: req.resolve.paths }
      ),
    }
  );
}

vi.mock('node:module', async (importOriginal) => {
  const actual = await importOriginal<typeof import('node:module')>();
  return {
    ...actual,
    createRequire: (url: string | URL) =>
      wrapRequireWithoutPresetTypescript(actual.createRequire(url)),
  };
});

describe('given @babel/preset-typescript is absent (pruned devDependency in a production install)', () => {
  it('when the node-loader module is imported, then it does not throw', async () => {
    await expect(importNodeLoader()).resolves.toBeDefined();
  });

  it('when load() runs for a non-TypeScript file, then it delegates to nextLoad without ever resolving @babel/preset-typescript', async () => {
    const { load } = await importNodeLoader();
    const nextLoad = vi
      .fn()
      .mockResolvedValue({ format: 'module', source: '', shortCircuit: true });

    await expect(
      load('file:///repo/dist/index.js', {}, nextLoad)
    ).resolves.toEqual({ format: 'module', source: '', shortCircuit: true });
    expect(nextLoad).toHaveBeenCalledWith('file:///repo/dist/index.js', {});
  });

  it('when load() runs for a .ts file, then it fails only at that point, proving resolution is lazy rather than eager', async () => {
    const { load } = await importNodeLoader();
    const nextLoad = vi.fn();

    await expect(
      load('file:///repo/src/index.ts', {}, nextLoad)
    ).rejects.toThrow(/preset-typescript/);
  });
});
