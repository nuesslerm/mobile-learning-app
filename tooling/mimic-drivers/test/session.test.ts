// Given a driver that holds a real server, When many interactions are asked, Then the server starts once, an interaction's own session
// prelude is the one the pristine server gave, `reset` starts a new server, and requests may be answered concurrently and out of order.
import assert from 'node:assert/strict'
import { PassThrough } from 'node:stream'
import { test } from 'node:test'
import { serve, type Driver } from '../src/protocol.ts'
import { shared } from '../src/session.ts'
import { t3 } from '../src/t3/index.ts'
import { fakeT3 } from './support/fake-t3.ts'

function counting() {
  const log: string[] = []
  const launch = async <R>(use: (n: number) => Promise<R>): Promise<R> => {
    const n = log.filter((l) => l.startsWith('start')).length + 1
    log.push(`start ${n}`)
    try {
      return await use(n)
    } finally {
      log.push(`stop ${n}`)
    }
  }
  return { log, launch }
}

test('Given a shared server, When many uses and a reset and a close come, Then it starts once per generation and every stop is awaited', async () => {
  const { log, launch } = counting()
  const s = shared(launch)
  const uses = await Promise.all([s.get(), s.get(), s.get()])
  assert.deepEqual(uses.map((u) => [u.server, u.generation]), [[1, 1], [1, 1], [1, 1]])
  await s.reset()
  assert.deepEqual(log, ['start 1', 'stop 1'], 'the reset returned only after the server was gone')
  assert.deepEqual((await s.get()).generation, 2)
  await s.close()
  assert.deepEqual(log, ['start 1', 'stop 1', 'start 2', 'stop 2'])
})

test('Given a launch that fails, When it is used, Then the failure reaches the caller and the next use tries again', async () => {
  let tries = 0
  const s = shared<number>(async () => {
    tries += 1
    throw new Error('no server')
  })
  await assert.rejects(s.get(), /no server/)
  await assert.rejects(s.get(), /no server/)
  assert.equal(tries, 2)
})

async function talk(driver: Driver, lines: string[], env: Record<string, string>): Promise<{ replies: any[]; at: number[] }> {
  const input = new PassThrough()
  const output = new PassThrough()
  const replies: any[] = []
  const at: number[] = []
  output.on('data', (d) => {
    for (const l of String(d).split('\n').filter(Boolean)) {
      replies.push(JSON.parse(l))
      at.push(Date.now())
    }
  })
  const saved = process.env.MIMIC_DRIVER_CONCURRENCY
  process.env.MIMIC_DRIVER_CONCURRENCY = env.MIMIC_DRIVER_CONCURRENCY
  try {
    const done = serve(driver, input, output)
    input.end(lines.join('\n') + '\n')
    await done
  } finally {
    if (saved === undefined) delete process.env.MIMIC_DRIVER_CONCURRENCY
    else process.env.MIMIC_DRIVER_CONCURRENCY = saved
  }
  return { replies, at }
}

function sleeper(events: string[]): Driver {
  return {
    name: 'sleeper',
    version: '1',
    protocol: 'mimic-driver/1',
    ops: { nap: async (input: { ms: number }) => (await new Promise((r) => setTimeout(r, input.ms)), { steps: [{ dir: 'ret', kind: 'json', payload: input.ms }] }) },
    reset: async () => void events.push('reset'),
    close: async () => void events.push('close'),
  }
}

test('Given a reset line, When the driver serves, Then it resets once the requests before it are answered and closes when the input ends', async () => {
  const events: string[] = []
  const lines = ['{"id":"a","op":"nap","input":{"ms":30}}', '{"reset":true}', '{"id":"b","op":"nap","input":{"ms":1}}']
  const { replies } = await talk(sleeper(events), lines, { MIMIC_DRIVER_CONCURRENCY: '4' })
  assert.deepEqual(replies.map((r) => r.id ?? Object.keys(r)[0]), ['a', 'reset', 'b'])
  assert.equal(replies[1].reset, 'ok')
  assert.deepEqual(events, ['reset', 'close'])
})

test('Given a concurrency of four, When four slow requests are sent, Then they overlap and each reply names its own request', async () => {
  const lines = [200, 160, 120, 80].map((ms, i) => `{"id":"r${i}","op":"nap","input":{"ms":${ms}}}`)
  const started = Date.now()
  const { replies } = await talk(sleeper([]), lines, { MIMIC_DRIVER_CONCURRENCY: '4' })
  assert.ok(Date.now() - started < 450, 'four requests of 560 ms in all took about the longest of them')
  assert.deepEqual(replies.map((r) => r.id), ['r3', 'r2', 'r1', 'r0'], 'answered as they finished')
  assert.deepEqual(replies.map((r) => r.steps[0].payload), [80, 120, 160, 200])
})

test('Given no concurrency setting, When requests are sent, Then they are answered one at a time in the order sent', async () => {
  const lines = [60, 1].map((ms, i) => `{"id":"r${i}","op":"nap","input":{"ms":${ms}}}`)
  const { replies } = await talk(sleeper([]), lines, {})
  assert.deepEqual(replies.map((r) => r.id), ['r0', 'r1'])
})

const withPrelude = () => ({ prelude: 'session' as const, settle_ms: 30, script: [{ 'ws-send': { _tag: 'Request', id: '7', tag: 'test.value', payload: {}, headers: [] } }, { 'ws-close': {} }] })

test('Given a session driver on a stand-in T3, When two interactions ask for the session prelude, Then the pairing is minted once, both start with the same prelude steps and each has its own answer', async () => {
  const fake = await fakeT3()
  const minted: string[] = []
  let starts = 0
  try {
    const launch = async <R>(use: (s: { baseUrl: string; workspace: string; mint: () => Promise<string> }) => Promise<R>): Promise<R> => {
      starts += 1
      return use({ baseUrl: fake.url, workspace: '/w', mint: async () => (fake.minted.push('CODE'), minted.push('CODE'), 'CODE') })
    }
    const driver = t3(launch, '1.0.0', { session: true })
    const a = await driver.ops.script!(withPrelude())
    const b = await driver.ops.script!(withPrelude())
    assert.equal(starts, 1, 'one server for both')
    assert.equal(minted.length, 1, 'one pairing credential: the prelude ran once')
    const opening = (steps: any[]) => steps.slice(0, steps.findIndex((s) => s.channel === 'ws:open') + 1)
    assert.deepEqual(opening(a.steps), opening(b.steps), 'the opening steps are the pristine prelude')
    for (const r of [a, b]) assert.ok(r.steps.some((s: any) => s.dir === 's2c' && s.channel === 'ws' && s.payload._tag === 'Exit' && s.rid === '7'), 'the interaction was answered on its own socket')
    assert.equal(fake.sockets.length, 3, 'the prelude socket and one socket per interaction')
    await driver.reset!()
    await driver.ops.script!(withPrelude())
    assert.equal(starts, 2, 'a reset gives a new server')
    assert.equal(minted.length, 2, 'and a new prelude')
  } finally {
    await fake.close()
  }
})
