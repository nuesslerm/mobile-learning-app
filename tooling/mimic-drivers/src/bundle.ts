// Bundles modules of a pinned reference tree with esbuild, resolving the third-party packages they import from this package's
// node_modules (the pnpm workspace lockfile's pins), never from whatever the reference tree or the repo root happens to hoist.
import { build, type Plugin } from 'esbuild'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { createRequire } from 'node:module'
import { packageDir } from './lock.ts'

const PINNED = /^(zod|tweetnacl|@noble\/hashes)(\/.*)?$/

const pinnedFrom = (resolveDir: string): Plugin => ({
  name: 'pinned-toolchain',
  setup(b) {
    b.onResolve({ filter: PINNED }, async (args) => {
      if (args.pluginData === 'pinned') return undefined
      return b.resolve(args.path, { kind: args.kind, resolveDir, pluginData: 'pinned' })
    })
  }
})

/** The exports of an entry file made of `export *` / `import` lines over modules in the reference tree. `resolveDir` is where the
 * pinned packages come from: this package by default, the reference's own workspace package when it ships what the app runs. */
export type Stub = { filter: RegExp; contents: string }

const stubbing = (stubs: Stub[]): Plugin => ({
  name: 'stubs',
  setup(b) {
    stubs.forEach((stub, i) => {
      b.onResolve({ filter: stub.filter }, (args) => ({ path: args.path, namespace: `stub-${i}` }))
      b.onLoad({ filter: /.*/, namespace: `stub-${i}` }, () => ({ contents: stub.contents, loader: 'js' }))
    })
  }
})

/** `stubs` replace modules the code imports for the app shell (storage, translation) and not for the wire. */
export async function bundleModule<T>(entrySource: string, resolveDir: string = packageDir, stubs: Stub[] = []): Promise<T> {
  const dir = mkdtempSync(join(tmpdir(), 'mimic-driver-'))
  try {
    const entry = join(dir, 'entry.ts')
    const out = join(dir, 'out.cjs')
    writeFileSync(entry, entrySource)
    await build({ entryPoints: [entry], bundle: true, platform: 'node', format: 'cjs', outfile: out, logLevel: 'error', plugins: [stubbing(stubs), pinnedFrom(resolveDir)] })
    return createRequire(import.meta.url)(out) as T
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
}
