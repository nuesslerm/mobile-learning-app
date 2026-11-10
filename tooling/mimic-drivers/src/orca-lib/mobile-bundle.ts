// Ops over the pinned Orca mobile-bundle contract files (manifest-contract, bundle-rpc-contract): the manifest a desktop builds for
// files, the windows it serves them in, and what its strict zod schemas accept (`reference_accepts`).
import { bundleModule } from '../bundle.ts'
import { ret, type Json, type Op, type Reply } from '../protocol.ts'
import { assertAtPin, orcaRoot } from './pin.ts'
import { materialize, sha256Hex, type FileSpec } from './synth.ts'

type Schema = { safeParse(v: unknown): { success: true } | { success: false; error: { issues: { code: string; path: PropertyKey[] }[] } } }

type Contract = {
  computeMobileWebBundleId(assets: unknown[]): string
  MOBILE_WEB_PAGE_VERSION: number
  MOBILE_WEB_BUNDLE_CHUNK_BYTES: number
  MOBILE_WEB_BUNDLE_ERROR_CODES: Json
  MobileWebBundleManifestSchema: Schema
  MobileWebBundleManifestResultSchema: Schema
  MobileWebBundleChunkResultSchema: Schema
  MobileWebBundleRangeResultSchema: Schema
}

let contract: Promise<Contract> | undefined

function load(): Promise<Contract> {
  contract ??= (async () => {
    assertAtPin()
    const shared = `${orcaRoot()}/src/shared/mobile-web-bundle`
    return bundleModule<Contract>(`export * from '${shared}/manifest-contract'\nexport * from '${shared}/bundle-rpc-contract'\n`)
  })()
  return contract
}

type ManifestInput = {
  files: FileSpec[]
  entrypoint: string
  minCompatibleRuntimeProtocolVersion: number
  runtimeProtocolVersion: number
  routes: Json
}

const byPath = (a: { path: string }, b: { path: string }) => (a.path < b.path ? -1 : 1)

async function built(input: ManifestInput) {
  const pin = assertAtPin()
  const c = await load()
  const files = materialize(input.files)
  const assets = [...files].map(([path, f]) => ({ path, sha256: sha256Hex(f.data), byteLength: f.data.length, contentType: f.type })).sort(byPath)
  const manifest = {
    schemaVersion: 1,
    buildId: c.computeMobileWebBundleId(assets),
    desktopVersion: pin.version,
    minCompatibleRuntimeProtocolVersion: input.minCompatibleRuntimeProtocolVersion,
    runtimeProtocolVersion: input.runtimeProtocolVersion,
    pageVersion: c.MOBILE_WEB_PAGE_VERSION,
    entrypoint: input.entrypoint,
    totalBytes: assets.reduce((n, a) => n + a.byteLength, 0),
    assets,
    routes: input.routes
  }
  const grid = c.MOBILE_WEB_BUNDLE_CHUNK_BYTES
  return { c, files, assets, manifest, grid, manifestResult: { manifest, chunkBytes: grid, rangeBytes: grid } }
}

const accepted = (s: Schema, v: unknown) => s.safeParse(v).success

const manifest: Op = async (input: ManifestInput): Promise<Reply> => {
  const { c, manifest, manifestResult } = await built(input)
  const ok = accepted(c.MobileWebBundleManifestSchema, manifest) && accepted(c.MobileWebBundleManifestResultSchema, manifestResult)
  return ret(manifestResult as Json, { status: 'ok', reference_accepts: ok })
}

/** Every window of every asset, in asset order: one step each, keyed by `path@offset`. */
const windows: Op = async (input: ManifestInput): Promise<Reply> => {
  const { c, files, assets, manifest, grid } = await built(input)
  const steps: Reply['steps'] = []
  let ok = true
  for (const a of assets) {
    const data = files.get(a.path)!.data
    for (let offset = 0; offset === 0 || offset < a.byteLength; offset += grid) {
      const piece = data.subarray(offset, offset + grid)
      const header = { buildId: manifest.buildId, path: a.path, offset, assetByteLength: a.byteLength, sha256: a.sha256, eof: offset + piece.length >= a.byteLength }
      const dataBase64 = piece.toString('base64')
      ok = ok && accepted(c.MobileWebBundleChunkResultSchema, { ...header, dataBase64 }) && accepted(c.MobileWebBundleRangeResultSchema, { ...header, encoding: 'identity', dataBase64 })
      steps.push({ dir: 'ret', channel: 'window', kind: 'json', rid: `${a.path}@${offset}`, payload: { ...header, windowSha256: sha256Hex(piece), windowLength: piece.length } })
    }
  }
  return { steps, effects: [], outcome: { status: 'ok', reference_accepts: ok } }
}

const errorCodes: Op = async (): Promise<Reply> => ret({ codes: (await load()).MOBILE_WEB_BUNDLE_ERROR_CODES, chunkBytes: (await load()).MOBILE_WEB_BUNDLE_CHUNK_BYTES })

const SCHEMAS = { manifest: 'MobileWebBundleManifestSchema', 'manifest-result': 'MobileWebBundleManifestResultSchema' } as const

function setAt(root: any, pointer: string, value: Json | undefined) {
  const keys = pointer.split('/').slice(1).map((k) => k.replace(/~1/g, '/').replace(/~0/g, '~'))
  const last = keys.pop()!
  const parent = keys.reduce((o, k) => o[k], root)
  if (value === undefined) delete parent[last]
  else parent[last] = value
}

/** A schema probe: the built manifest (or manifest result) with `set` pointers replaced and `remove` pointers deleted, then parsed by
 * the pinned schema. The rejection is the recorded behaviour, so the outcome is ok and `reference_accepts` says what the schema did. */
const probe: Op = async (input: ManifestInput & { schema: keyof typeof SCHEMAS; set?: Record<string, Json>; remove?: string[] }): Promise<Reply> => {
  const { c, manifest, manifestResult } = await built(input)
  const target: any = JSON.parse(JSON.stringify(input.schema === 'manifest' ? manifest : manifestResult))
  for (const [pointer, value] of Object.entries(input.set ?? {})) setAt(target, pointer, value)
  for (const pointer of input.remove ?? []) setAt(target, pointer, undefined)
  const result = (c[SCHEMAS[input.schema]] as Schema).safeParse(target)
  const issues = result.success ? [] : result.error.issues.map((i) => ({ code: i.code, path: i.path.join('/') }))
  return ret({ accepted: result.success, issues }, { status: 'ok', reference_accepts: result.success })
}

export const ops: Record<string, Op> = { 'bundle.manifest': manifest, 'bundle.windows': windows, 'bundle.error-codes': errorCodes, 'bundle.probe': probe }
