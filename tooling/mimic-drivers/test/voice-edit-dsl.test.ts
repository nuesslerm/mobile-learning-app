// Given the cassette of the voice-edit DSL recorded through the voice-edit-dsl driver,
// When it is turned back into the format of the old generate-dsl.ts (dsl.json), and the driver is asked its recorded inputs again,
// Then every value is the one the old file held (the import diff that licensed deleting it), and the driver answers what the cassette says.
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { spawn } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { test } from 'node:test'
import { packageDir, repoRoot, toolchainLockHash } from '../src/lock.ts'

type Interaction = { id: string; op: string; input: any; tags?: string[]; steps: { payload: any }[] }

const cassette = join(repoRoot, 'backend/remote-chat-agent/rs/tests/cassettes/voice-edit-dsl/interactions.jsonl')
const all: Interaction[] = readFileSync(cassette, 'utf8').trim().split('\n').map((l) => JSON.parse(l))
// the property cases (and their shrink ladders) were generated after the old file was retired: it never held them
const rows = all.filter((r) => !r.tags?.includes('property'))
const of = (op: string) => rows.filter((r) => r.op === op)

// sha256 of the canonical (keys sorted, compact) JSON of the old dsl.json, taken before generate-dsl.ts and the file were deleted
const OLD = 'c28199161c358e53745db47813094e76953c3de82c0e16e2832a8c12300dac4b'

const sorted = (v: any): any => (Array.isArray(v) ? v.map(sorted) : v && typeof v === 'object' ? Object.fromEntries(Object.keys(v).sort().map((k) => [k, sorted(v[k])])) : v)

/** The dsl.json the old generate-dsl.ts wrote, rebuilt from the cassette. */
function oldGolden() {
  const out = (r: Interaction) => r.steps[0]!.payload
  return {
    validate: of('dsl.validate').map((r) => ({ ...r.input, expected: out(r) })),
    blocks: of('dsl.blocks').map((r) => ({ code: r.input.code, language: r.input.language, alternative: r.input.alternative, expected: out(r) })),
    repair: of('dsl.repair').map((r) => ({ text: r.input.text, expected: out(r) })),
    limits: of('dsl.limits').map((r) => ({ transcript: r.input.transcript, expected: out(r) })),
    normalize: of('dsl.normalize').map((r) => ({ input: r.input.input, expected: out(r) }))
  }
}

function ask(requests: Interaction[]): Promise<any[]> {
  return new Promise((resolve, reject) => {
    const child = spawn('node', [join(packageDir, 'src/cli.ts'), 'serve', 'voice-edit-dsl'], { env: { ...process.env, NODE_NO_WARNINGS: '1' } })
    let out = ''
    let err = ''
    child.stdout.on('data', (d) => (out += d))
    child.stderr.on('data', (d) => (err += d))
    child.on('close', () => (out.trim() ? resolve(out.trim().split('\n').map((l) => JSON.parse(l))) : reject(new Error(`no reply: ${err}`))))
    child.stdin.end(requests.map((r) => JSON.stringify({ id: r.id, op: r.op, input: r.input })).join('\n') + '\n')
  })
}

test('dsl.json rebuilt from the cassette holds the old values', () => {
  assert.equal(createHash('sha256').update(JSON.stringify(sorted(oldGolden()))).digest('hex'), OLD)
})

test('the driver answers each recorded input of the small ops with the recorded answer', async () => {
  const small = rows.filter((r) => r.op !== 'dsl.validate' && r.op !== 'dsl.blocks')
  assert.ok(small.length > 0)
  const replies = await ask(small)
  assert.deepEqual(replies.map((r) => [r.id, r.steps[0].payload]), small.map((r) => [r.id, r.steps[0]!.payload]))
})

test('an op the driver does not have is an error naming the ops it has, and the next request is still served', async () => {
  const [first, second] = await ask([{ id: 'x', op: 'dsl.nope', input: {}, steps: [] }, of('dsl.limits')[0]!])
  assert.match(first.error, /unknown op dsl\.nope.*dsl\.limits/)
  assert.equal(second.id, of('dsl.limits')[0]!.id)
})

test('the driver answers the recorded property cases and their shrink ladders with the recorded answers', async () => {
  const property = all.filter((r) => r.tags?.includes('property'))
  const ladder = property.filter((r) => r.tags?.includes('shrink'))
  assert.ok(property.length > ladder.length && ladder.length > 0)
  const replies = await ask(property)
  assert.deepEqual(replies.map((r) => [r.id, r.steps[0].payload]), property.map((r) => [r.id, r.steps[0]!.payload]))
  for (const step of ladder) {
    const base = all.find((r) => step.tags!.includes(`shrink-of:${r.id}`))!
    assert.ok(JSON.stringify(step.input).length <= JSON.stringify(base.input).length, `${step.id} is not smaller than ${base.id}`)
  }
})

test('the toolchain hash follows the zod the library itself is locked to, and only that of the library', () => {
  const lock = readFileSync(join(repoRoot, 'pnpm-lock.yaml'), 'utf8')
  const pins = { importer: 'frontend/libraries/voice-edit-dsl', packages: ['zod'] }
  const version = /frontend\/libraries\/voice-edit-dsl:\n(?:.*\n)*?      zod:\n        specifier: .*\n        version: (\S+)/.exec(lock)![1]!
  const moved = lock.replace(new RegExp(`(  zod@${version.replace(/\./g, '\\.')}:\n    resolution: \\{integrity: sha512-)`), '$1X')
  assert.notEqual(moved, lock)
  assert.notEqual(toolchainLockHash(moved, pins), toolchainLockHash(lock, pins))
  const other = lock.replace(/(frontend\/libraries\/voice-edit-dsl:\n(?:.*\n)*?      vitest:\n        specifier: .*\n        version: )/, '$1x')
  assert.equal(toolchainLockHash(other, pins), toolchainLockHash(lock, pins))
})
