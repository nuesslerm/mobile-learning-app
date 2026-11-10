// Given the T3 driver and a stand-in server (HTTP routes and a WebSocket that answers frames the way the Effect RPC framing does),
// When it runs the script of an interaction, Then it reports the wire raw, in order, with the values of the run filled in and the workspace
// folder named `<workspace>`; and the release it would start is only ever bytes the pin names.
import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { t3 } from '../src/t3/index.ts'
import { fakeT3 } from './support/fake-t3.ts'
import { assetName, credentialOf, ensureRuntime, parseSha256Sums, sha256, type Pin } from '../src/t3/release.ts'

const script = [
  { http: { method: 'POST', path: '/oauth/token', headers: { accept: '*/*', 'content-type': 'application/x-www-form-urlencoded' }, body: { subject_token: '{credential}', client_os: 'linux' } } },
  { http: { method: 'POST', path: '/api/auth/websocket-ticket/rpc', headers: { accept: '*/*', authorization: 'Bearer {access_token}' }, body: null } },
  { 'ws-open': { path: '/ws?orchestrationProtocol=2&wsTicket={ticket}' } },
  { 'ws-send': { _tag: 'Ping' } },
  { 'ws-send': { _tag: 'Request', id: '1', tag: 'subscribeShell', payload: { workspaceRoot: '{workspace}' }, headers: [] } },
  { 'ws-send': { _tag: 'Request', id: '2', tag: 'server.probe', payload: {}, headers: [] } },
  { 'ws-close': { code: 1000, reason: 'done' } },
]

test('Given a script of calls and a socket session, When the driver runs it, Then the raw wire comes back in order with this run\'s values filled in', async () => {
  const fake = await fakeT3()
  try {
    const driver = t3(async (use) => use({ baseUrl: fake.url, workspace: '/tmp/some-run/workspace', mint: async () => (fake.minted.push('CODE-0001'), 'CODE-0001') }), '9.9.9')
    const reply = await driver.ops.script!({ script, settle_ms: 40 })
    const at = (channel: string, dir: string, n = 0) => reply.steps.filter((s) => s.channel === channel && s.dir === dir)[n]!
    const call = at('http', 'call').payload as { body: string }
    assert.equal(call.body, 'subject_token=CODE-0001&client_os=linux', 'the form carries the credential this run minted')
    assert.equal((at('http', 'ret').payload as { status: number }).status, 200)
    assert.equal((at('http', 'call', 1).payload as { headers: Record<string, string> }).headers.authorization, 'Bearer access-token-1')
    assert.deepEqual(at('ws:open', 's2c').payload, { path: '/ws?orchestrationProtocol=2&wsTicket=ticket-0001', status: 101 })
    const frames = reply.steps.filter((s) => s.channel === 'ws').map((s) => `${s.dir}:${(s.payload as { _tag: string })._tag}:${s.rid ?? '-'}`)
    assert.deepEqual(frames, ['c2s:Ping:-', 's2c:Pong:-', 'c2s:Request:1', 's2c:Chunk:1', 'c2s:Ack:1', 'c2s:Request:2', 's2c:Exit:2'], 'a chunk is acknowledged as the recorded client did')
    assert.equal(JSON.stringify(reply.steps).includes('/tmp/some-run'), false, 'the run\'s workspace folder is reported as <workspace>')
    assert.ok(JSON.stringify(reply.steps).includes('<workspace>'))
    const closes = reply.steps.filter((s) => s.channel === 'ws:close').map((s) => `${s.dir}:${(s.payload as { tcp: boolean }).tcp}`)
    assert.deepEqual(closes, ['c2s:false', 's2c:false', 's2c:true'], 'the close handshake, then the connection dropping')
    assert.equal(driver.targetVersion, '9.9.9')
  } finally {
    await fake.close()
  }
})

test('Given a credential the server does not know, When the script exchanges it, Then the refusal is data in the steps, not an error', async () => {
  const fake = await fakeT3()
  try {
    const driver = t3(async (use) => use({ baseUrl: fake.url, workspace: '/w', mint: async () => 'NOT-MINTED' }))
    const reply = await driver.ops.script!({ script: [script[0]!], settle_ms: 10 })
    assert.equal((reply.steps.find((s) => s.dir === 'ret')!.payload as { status: number }).status, 401)
  } finally {
    await fake.close()
  }
})

test('Given a server that is gone when the script calls it, When the driver runs it, Then the interaction ends there with the transport error as data and the outcome says so', async () => {
  const fake = await fakeT3()
  const base = fake.url
  await fake.close()
  const driver = t3(async (use) => use({ baseUrl: base, workspace: '/w', mint: async () => 'CODE' }))
  const reply = await driver.ops.script!({ script: [script[0]!], settle_ms: 10 })
  assert.deepEqual(reply.outcome, { status: 'error', code: 'transport' })
  assert.equal(reply.steps.at(-1)!.channel, 'driver')
  assert.ok((reply.steps.at(-1)!.payload as { transport_error: string }).transport_error.length > 0)
})

function release(bytes: Buffer, sums: string, platform = 'linux-x64') {
  const pin: Pin = { version: '1.2.3', commit: 'c', release: 'https://example.test/rel', platforms: { [platform]: { bytes: bytes.length, sha256: sha256(bytes) } } }
  const hits: string[] = []
  const get = async (url: string) => {
    hits.push(url)
    return url.endsWith('SHA256SUMS') ? Buffer.from(sums) : bytes
  }
  return { pin, hits, get, name: assetName(pin.version, platform) }
}

function tarball(): Buffer {
  const dir = mkdtempSync(join(tmpdir(), 't3-pack-'))
  mkdirSync(join(dir, 'pkg'))
  writeFileSync(join(dir, 'pkg', 't3'), 'binary')
  spawnSync('tar', ['czf', join(dir, 'x.tgz'), '-C', dir, 'pkg'])
  const bytes = readFileSync(join(dir, 'x.tgz'))
  rmSync(dir, { recursive: true })
  return bytes
}

test('Given a release the pin and its SHA256SUMS both name, When the runtime is ensured twice, Then it is unpacked and the second call downloads nothing', async () => {
  const bytes = tarball()
  const cache = mkdtempSync(join(tmpdir(), 't3-cache-'))
  const rel = release(bytes, `${sha256(bytes)}  ${assetName('1.2.3', 'linux-x64')}\n`)
  const first = await ensureRuntime({ pin: rel.pin, platform: 'linux-x64', cache, get: rel.get })
  assert.equal(first.downloaded, true)
  assert.equal(readFileSync(join(first.dir, 't3'), 'utf8'), 'binary')
  const again = await ensureRuntime({ pin: rel.pin, platform: 'linux-x64', cache, get: rel.get })
  assert.equal(again.downloaded, false)
  assert.equal(rel.hits.length, 2, 'SHA256SUMS and the tarball were fetched once')
  rmSync(cache, { recursive: true })
})

test('Given bytes the pin does not name or sums that disagree with it, When the runtime is ensured, Then it is refused naming which, and nothing is kept', async () => {
  const bytes = tarball()
  const cache = mkdtempSync(join(tmpdir(), 't3-cache-'))
  const good = release(bytes, '')
  const tampered = { ...good, get: async (url: string) => (url.endsWith('SHA256SUMS') ? Buffer.from(`${sha256(bytes)}  ${good.name}\n`) : Buffer.from('other bytes')) }
  await assert.rejects(ensureRuntime({ pin: good.pin, platform: 'linux-x64', cache, get: tampered.get }), /does not match the pin/)
  const wrongSums = release(bytes, `${'0'.repeat(64)}  ${good.name}\n`)
  await assert.rejects(ensureRuntime({ pin: wrongSums.pin, platform: 'linux-x64', cache, get: wrongSums.get }), /disagrees with the pin/)
  await assert.rejects(ensureRuntime({ pin: good.pin, platform: 'linux-arm64', cache, get: good.get }), /no platform linux-arm64/)
  assert.deepEqual(readdirSync(cache), [], 'no tarball or runtime was kept')
  rmSync(cache, { recursive: true })
})

test('Given what the pairing command printed, When the credential is read, Then the first code-like string is it and anything else names the keys it saw', () => {
  assert.equal(credentialOf('{"token":"abc123","expiresAt":"x"}'), 'abc123')
  assert.throws(() => credentialOf('{"id":1}'), /keys: id/)
  assert.equal(parseSha256Sums(`${'a'.repeat(64)} *file.tgz\n`).get('file.tgz'), 'a'.repeat(64))
})

test('Given a script whose later entries name values an earlier answer teaches, When the driver runs it, Then the value is used, and an entry whose value never came is reported, not sent', async () => {
  const fake = await fakeT3()
  try {
    const driver = t3(async (use) => use({ baseUrl: fake.url, workspace: '/w', mint: async () => (fake.minted.push('CODE-0002'), 'CODE-0002') }))
    const entries = [
      { ...script[0]!, capture: { token: { from: ['/access_token'] }, missing: { from: ['/nope'] } } },
      { http: { method: 'POST', path: '/api/auth/websocket-ticket/rpc', headers: { accept: '*/*', authorization: 'Bearer {token}' }, body: null } },
      { 'ws-open': { path: '/ws?wsTicket={ticket}' } },
      { 'ws-send': { _tag: 'Request', id: 'v1', tag: 'test.value', payload: {}, headers: [] }, capture: { run: { from: ['/exit/value/runs/0/runId'] } } },
      { 'ws-send': { _tag: 'Request', id: 'v2', tag: 'test.use', payload: { runId: '{run}' }, headers: [] } },
      { 'ws-send': { _tag: 'Request', id: 'v3', tag: 'test.use', payload: { other: '{missing}' }, headers: [] } },
      { 'ws-close': {} }
    ]
    const reply = await driver.ops.script!({ script: entries, settle_ms: 40 })
    const sent = reply.steps.filter((s) => s.dir === 'c2s' && s.channel === 'ws').map((s) => s.payload as { id?: string; payload?: unknown })
    assert.deepEqual(sent.find((p) => p.id === 'v2')?.payload, { runId: 'run-7' }, 'the value the answer carried reaches the next request')
    assert.ok(!sent.some((p) => p.id === 'v3'), 'a request whose value never came is not sent')
    const refused = reply.steps.filter((s) => s.channel === 'driver').map((s) => s.payload as { client_error?: string })
    assert.deepEqual(refused.map((p) => p.client_error), ['unbound variable missing'])
    assert.equal(reply.outcome?.status, 'ok', 'an unbound value is a finding in the steps, not a transport failure')
    assert.ok(fake.minted.length === 1)
  } finally {
    await fake.close()
  }
})

test('Given a setup request the server answers after the quiet period, When the driver runs it, Then the answer is a step of the run, and without the wait it is lost to the close', async () => {
  const fake = await fakeT3()
  const open = [script[0]!, script[1]!, script[2]!]
  const slow = (extra: Record<string, unknown>) => ({ 'ws-send': { _tag: 'Request', id: 'slow', tag: 'test.slow', payload: {}, headers: [] }, ...extra })
  const run = async (entries: typeof script) => {
    const driver = t3(async (use) => use({ baseUrl: fake.url, workspace: '/tmp/w', mint: async () => 'CODE-0001' }), '9.9.9')
    return (await driver.ops.script!({ script: [...open, ...entries, { 'ws-close': { code: 1000, reason: 'done' } }], settle_ms: 40 })).steps
  }
  const answered = (steps: Awaited<ReturnType<typeof run>>) => steps.some((s) => s.channel === 'ws' && s.dir === 's2c' && s.rid === 'slow')
  try {
    assert.equal(answered(await run([slow({ await: 'exit' })])), true)
    assert.equal(answered(await run([slow({})])), false, 'the control: the quiet period alone does not wait for it')
  } finally {
    await fake.close()
  }
})
