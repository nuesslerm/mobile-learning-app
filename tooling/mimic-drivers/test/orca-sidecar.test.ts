// Given the Orca codec sidecar over the phone's pinned client and a scripted double of an Orca host,
// When control lines arrive (hello, tap, connect, request, subscribe, unsubscribe, close, quit),
// Then each is answered by id, a host answer comes back minus the minted id, a wrong token is a state, stream frames and the close are events, and quit ends the loop.
import assert from 'node:assert/strict'
import { PassThrough } from 'node:stream'
import { test } from 'node:test'
import { startHostDouble } from '../src/orca/host-double.ts'
import { jsonSafe, serveSidecar, sidecar } from '../src/orca/sidecar.ts'
import { loadPhone } from '../src/orca/phone.ts'

const METHODS = { 'files.list': { result: { entries: [] } } }

const deps = (double: object, events: unknown[] = []) => ({ launch: async () => (use: (t: any) => Promise<unknown>) => use(double), load: loadPhone, emit: (e: unknown) => void events.push(e) })

test('a request before connect says so, and after connect the host answers without its minted id', async () => {
  const double = await startHostDouble({ methods: METHODS })
  try {
    const side = sidecar(deps(double) as any)

    assert.deepEqual(await side.handle({ op: 'request', input: { method: 'files.list' } }), { client_error: 'not connected' })
    assert.deepEqual(await side.handle({ op: 'connect', input: { timeout_ms: 5000 } }), { state: 'connected' })
    const answer = (await side.handle({ op: 'request', input: { method: 'files.list', params: {} } })) as any
    assert.deepEqual(answer.response.result, { entries: [] })
    assert.ok(!('id' in answer.response))
    assert.deepEqual(await side.handle({ op: 'quit' }), {})
  } finally {
    await double.close()
  }
})

test('hello names the protocol and the pinned version', async () => {
  const side = sidecar(deps({}) as any)

  const { hello } = (await side.handle({ op: 'hello' })) as any

  assert.equal(hello.protocol, 'mimic-orca-phone/1')
  assert.ok(hello.target_version)
})

test('a token the host does not know is a state, not an exception', async () => {
  const double = await startHostDouble({ methods: METHODS })
  try {
    const side = sidecar(deps({ ...double, deviceToken: 'wrong' }) as any)

    const { state } = (await side.handle({ op: 'connect', input: { timeout_ms: 5000 } })) as { state: string }

    assert.notEqual(state, 'connected')
    await side.handle({ op: 'quit' })
  } finally {
    await double.close()
  }
})

test('close is a reply, and a close event only when the close tap is on; a stream asked of an unconnected client is a client error', async () => {
  const events: unknown[] = []
  const side = sidecar(deps({}, events) as any)

  assert.deepEqual(await side.handle({ op: 'subscribe', input: { method: 'notifications.subscribe' } }), { client_error: 'not connected' })
  assert.deepEqual(await side.handle({ op: 'close' }), { state: 'disconnected' })
  assert.deepEqual(events, [])
  await side.handle({ op: 'tap', input: { close: true } })
  await side.handle({ op: 'close' })
  assert.deepEqual(events, [{ event: 'close', code: null, reason: '' }])
})

test('bytes inside a frame become base64 and everything else stays', () => {
  assert.deepEqual(jsonSafe({ a: new Uint8Array([1, 2, 3]), b: [1, { c: undefined }] }), { a: 'AQID', b: [1, { c: null }] })
})

test('the line loop answers every control line by its id and ends after quit', async () => {
  const double = await startHostDouble({ methods: METHODS })
  try {
    const input = new PassThrough()
    const out: string[] = []
    const done = serveSidecar(input, (line) => out.push(line), deps(double) as any)

    input.write('{"id":"1","op":"connect","input":{"timeout_ms":5000}}\n{"id":"2","op":"request","input":{"method":"files.list","params":{}}}\n{"id":"3","op":"quit"}\n')
    await done

    assert.equal(out.length, 3)
    assert.deepEqual(JSON.parse(out[0]!), { id: '1', ok: { state: 'connected' } })
    assert.equal(JSON.parse(out[1]!).ok.response.result.entries.length, 0)
  } finally {
    await double.close()
  }
})
