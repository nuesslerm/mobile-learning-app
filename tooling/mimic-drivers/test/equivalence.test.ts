// Given the two cassette sets recorded from the pinned Orca code through the orca-lib driver,
// When they are turned back into the formats of the old generate.mjs scripts (goldens.json, bundle/manifest.json, vectors.json),
// Then every value is the one the old files held: the import diff that licenses deleting the generators and their committed output.
// While the old files are in git they are compared directly; after they are deleted, by the hash of their canonical form (below).
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { test } from 'node:test'
import { repoRoot } from '../src/lock.ts'
import { generate, sha256Hex, type FileSpec } from '../src/orca-lib/synth.ts'

const mobile = join(repoRoot, 'clis/orca-serve/map-mobile/fixtures')
const crypto = join(repoRoot, 'clis/orca-serve/crypto/vectors')

// sha256 of the canonical (keys sorted, compact) JSON of each old file, taken before the generators were deleted
const OLD = {
  goldens: '57eabfbe8f91abab3b1387653f889ee51a90a246147c2a04bb5926535aa86314',
  manifest: '9e9a9a26adb1a82f79d6c5ec334d83b1dd60f1e9dbce3885c13d0e0aaed4c3c7',
  vectors: '04cb627e27f9c24ccdbcd0af4d734d17e160e7fa0eefb20592c52d6a57de5442'
}

const sorted = (v: any): any => (Array.isArray(v) ? v.map(sorted) : v && typeof v === 'object' ? Object.fromEntries(Object.keys(v).sort().map((k) => [k, sorted(v[k])])) : v)
const canon = (v: unknown) => JSON.stringify(sorted(v))
const hash = (v: unknown) => createHash('sha256').update(canon(v)).digest('hex')
const json = (path: string) => JSON.parse(readFileSync(path, 'utf8'))

type Interaction = { id: string; input: any; steps: { rid?: string; payload: any }[]; outcome: { status: string; reference_accepts?: boolean } }

function set(dir: string): Map<string, Interaction> {
  return new Map(readFileSync(join(dir, 'interactions.jsonl'), 'utf8').trim().split('\n').map((l) => JSON.parse(l) as Interaction).map((i) => [i.id, i]))
}

const payload = (s: Map<string, Interaction>, id: string) => s.get(id)!.steps[0]!.payload

/** The goldens.json the old fixtures/generate.mjs wrote, rebuilt from the cassette. */
function oldGoldens(s: Map<string, Interaction>) {
  const manifestResult = payload(s, 'manifest')
  const codes = payload(s, 'error-codes')
  return {
    generatedFrom: 'references/orca @ pinned commit; zod 4.6.7',
    errorCodes: codes.codes,
    chunkBytes: codes.chunkBytes,
    manifestResult,
    windows: s.get('windows')!.steps.map((st) => st.payload)
  }
}

/** The vectors.json the old vectors/generate.mjs wrote. */
function oldVectors(s: Map<string, Interaction>) {
  const hex = (id: string) => payload(s, id).hex
  return {
    generatedFrom: 'references/orca @ pinned commit; tweetnacl 1.0.3, @noble/hashes 1.8.0',
    clientSecretHex: payload(s, 'keypair-client').secretHex,
    clientPublicHex: payload(s, 'keypair-client').publicHex,
    serverSecretHex: payload(s, 'keypair-server').secretHex,
    serverPublicHex: payload(s, 'keypair-server').publicHex,
    clientNonceHex: hex('nonce-client'),
    desktopNonceHex: hex('nonce-desktop'),
    boxSharedKeyHex: hex('box-shared-key'),
    pinnedV2Vector: payload(s, 'pinned-v2-vector'),
    handshakes: [payload(s, 'handshake-direct'), payload(s, 'handshake-relay')],
    legacy: payload(s, 'legacy'),
    offer: payload(s, 'offer')
  }
}

const bundle = set(join(mobile, 'orca-mobile-bundle'))
const vectors = set(join(crypto, 'orca-crypto'))

test('every reference interaction was accepted by the pinned code, and the schema probes are recorded as data', () => {
  for (const [id, i] of [...bundle, ...vectors]) {
    assert.equal(i.outcome.status, 'ok', id)
    if (!id.startsWith('probe-')) assert.equal(i.outcome.reference_accepts, true, `${id}: the pinned schemas accepted what the old generator asserted`)
  }
  const probes = [...bundle].filter(([id]) => id.startsWith('probe-'))
  assert.ok(probes.length > 0)
  for (const [id, i] of probes) assert.equal(i.outcome.reference_accepts, false, `${id}: a damaged manifest must be rejected`)
})

test('goldens.json rebuilt from the cassette holds the old values', () => {
  const rebuilt = oldGoldens(bundle)
  const old = join(mobile, 'goldens.json')
  if (existsSync(old)) assert.deepEqual(sorted(rebuilt), sorted(json(old)))
  assert.equal(hash(rebuilt), OLD.goldens)
})

test('the synthetic bundle files are the committed ones, byte for byte, and the manifest names their hashes', () => {
  const input = bundle.get('manifest')!.input
  const manifest = payload(bundle, 'manifest').manifest
  const old = join(mobile, 'bundle')
  for (const f of input.files as FileSpec[]) {
    const data = generate(f.gen)
    assert.equal(manifest.assets.find((a: any) => a.path === f.path).sha256, sha256Hex(data), f.path)
    if (existsSync(join(old, f.path))) assert.deepEqual(data, readFileSync(join(old, f.path)), f.path)
  }
  if (existsSync(join(old, 'manifest.json'))) assert.deepEqual(sorted(manifest), sorted(json(join(old, 'manifest.json'))))
  assert.equal(hash(manifest), OLD.manifest)
})

test('vectors.json rebuilt from the cassette holds the old values', () => {
  const rebuilt = oldVectors(vectors)
  const old = join(crypto, 'vectors.json')
  if (existsSync(old)) assert.deepEqual(sorted(rebuilt), sorted(json(old)))
  assert.equal(hash(rebuilt), OLD.vectors)
})
