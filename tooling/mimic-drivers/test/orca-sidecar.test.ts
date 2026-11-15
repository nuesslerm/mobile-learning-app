// Given the Orca codec sidecar over the phone's pinned client and a scripted double of an Orca host,
// When messages arrive as JSON lines (connect, request, disconnect, close),
// Then the host's answers come back minus the minted id, a failure is the client's own text, a wrong token is a state, and close ends the loop.
import assert from 'node:assert/strict'
import { PassThrough } from 'node:stream'
import { test } from 'node:test'
import { startHostDouble } from '../src/orca/host-double.ts'
import { serveSidecar, sidecar } from '../src/orca/sidecar.ts'
import { loadPhone } from '../src/orca/phone.ts'

const METHODS = { 'files.list': { result: { entries: [] } } }

test('a request before connect says so, and after connect the host answers without its minted id', async () => {
  const double = await startHostDouble({ methods: METHODS })
  try {
    const side = sidecar({ launch: async () => (use) => use(double), load: loadPhone })

    assert.deepEqual(await side.handle({ op: 'request', method: 'files.list' }), { ok: false, error: 'not connected' })
    assert.deepEqual(await side.handle({ op: 'connect', timeout_ms: 5000 }), { state: 'connected' })
    const answer = (await side.handle({ op: 'request', method: 'files.list', params: {} })) as any
    assert.equal(answer.ok, true)
    assert.deepEqual(answer.payload.result, { entries: [] })
    assert.ok(!('id' in answer.payload))
    assert.deepEqual(await side.handle({ op: 'close' }), {})
  } finally {
    await double.close()
  }
})

test('a token the host does not know is a state, not an exception', async () => {
  const double = await startHostDouble({ methods: METHODS })
  try {
    const side = sidecar({ launch: async () => (use) => use({ ...double, deviceToken: 'wrong' }), load: loadPhone })

    const { state } = (await side.handle({ op: 'connect', timeout_ms: 5000 })) as { state: string }

    assert.notEqual(state, 'connected')
    await side.handle({ op: 'close' })
  } finally {
    await double.close()
  }
})

test('the line loop answers one line per message and ends after close', async () => {
  const double = await startHostDouble({ methods: METHODS })
  try {
    const input = new PassThrough()
    const out: string[] = []
    const done = serveSidecar(input, (line) => out.push(line), { launch: async () => (use) => use(double), load: loadPhone })

    input.write('{"op":"connect","timeout_ms":5000}\n{"op":"request","method":"files.list","params":{}}\n{"op":"close"}\n')
    await done

    assert.equal(out.length, 3)
    assert.deepEqual(JSON.parse(out[0]!), { state: 'connected' })
    assert.equal(JSON.parse(out[1]!).ok, true)
  } finally {
    await double.close()
  }
})
