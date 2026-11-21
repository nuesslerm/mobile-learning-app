// Given the Orca ws driver, a scripted double of an Orca host (the pinned desktop e2ee channel behind a WebSocket) and a fake orcad machine,
// When the driver runs request scripts on the phone's pinned transport and the provisioner builds and launches an orcad at the pin,
// Then the driver reports the decrypted request and response frames, a wrong credential or a dead endpoint as an error outcome, and the
// provisioner refuses a checkout off the pin, runs the build recipe once, and pairs through the ready line.
import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { createInterface } from 'node:readline'
import { join } from 'node:path'
import { test } from 'node:test'
import { packageDir } from '../src/lock.ts'
import { startHostDouble, type Methods } from '../src/orca/host-double.ts'
import { buildRecipe, orcadLauncher, readyTarget, type Command, type Ports } from '../src/orca/orcad.ts'
import { targetFromEnv } from '../src/orca/target.ts'
import { readPin } from '../src/orca-lib/pin.ts'

const METHODS: Methods = {
  'files.list': { result: { entries: [{ name: 'a.txt', kind: 'file' }] } },
  'files.read': { error: { code: 'not_found', message: 'no such file' } }
}

test('a half-set target in the environment is refused by name, an empty one is no target', async () => {
  assert.equal(await targetFromEnv({}), null)
  await assert.rejects(() => targetFromEnv({ ORCA_ENDPOINT: 'ws://x' }), /go together/)
})

test('the pinned pairing code of a ready line becomes the target', async () => {
  const double = await startHostDouble({ methods: METHODS })
  try {
    const offer = { v: 2, endpoint: double.endpoint, deviceToken: double.deviceToken, publicKeyB64: double.serverPublicKeyB64 }
    const url = `orca://pair?code=${Buffer.from(JSON.stringify(offer)).toString('base64url')}`
    const lines = (async function* () {
      yield 'booting orcad'
      yield JSON.stringify({ type: 'orca_server_log', message: 'x' })
      yield JSON.stringify({ type: 'orca_server_ready', advertisedEndpoint: double.endpoint, pairing: { url } })
    })()
    assert.deepEqual(await readyTarget(lines), { endpoint: double.endpoint, deviceToken: double.deviceToken, serverPublicKeyB64: double.serverPublicKeyB64 })
  } finally {
    await double.close()
  }
  await assert.rejects(() => readyTarget((async function* () { yield 'nothing' })()), /exited before/)
})

// A fake machine: "orcad" is the host double process itself, started by the provisioner's start port.
function fakeMachine(head: string, state: { built: boolean; ran: Command[]; stopped: number }): Ports {
  return {
    head: () => head,
    run: async (c) => {
      state.ran.push(c)
      if (c.args.includes('build:orcad')) state.built = true
      return 0
    },
    built: () => (state.built ? { node: process.execPath, entry: join(packageDir, 'src/orca/host-double.ts') } : null),
    port: async () => 6800,
    start: (node, args) => {
      const child = spawn(node, [args[0]!], { env: { ...process.env, NODE_NO_WARNINGS: '1', ORCA_DOUBLE_METHODS: JSON.stringify(METHODS) }, stdio: ['pipe', 'pipe', 'inherit'] })
      return {
        stdout: createInterface({ input: child.stdout }),
        stop: () =>
          new Promise<void>((resolve) => {
            state.stopped++
            child.on('close', () => resolve())
            child.stdin.end()
          })
      }
    }
  }
}

test('the provisioner builds once at the pin, launches orcad, pairs, and stops it', async () => {
  const state = { built: false, ran: [] as Command[], stopped: 0 }
  const launch = orcadLauncher('/checkout', fakeMachine(readPin().commit, state))
  const paired = await launch(async (target) => target.endpoint)
  assert.match(paired, /^ws:/)
  assert.deepEqual(state.ran, buildRecipe('/checkout'))
  assert.equal(state.stopped, 1)
  await launch(async () => undefined)
  assert.equal(state.ran.length, buildRecipe('/checkout').length, 'a built checkout is not built again')
})

test('the provisioner refuses a checkout that is not at the pin and runs nothing', async () => {
  const state = { built: false, ran: [] as Command[], stopped: 0 }
  await assert.rejects(() => orcadLauncher('/checkout', fakeMachine('0'.repeat(40), state))(async () => undefined), /pin names/)
  assert.deepEqual(state.ran, [])
})

test('a failing build step stops the recipe and names the step', async () => {
  const state = { built: false, ran: [] as Command[], stopped: 0 }
  const machine = {
    ...fakeMachine(readPin().commit, state),
    run: async (c: Command) => {
      state.ran.push(c)
      return 3
    }
  }
  await assert.rejects(() => orcadLauncher('/checkout', machine)(async () => undefined), /install --frozen-lockfile exited 3/)
  assert.equal(state.ran.length, 1)
})

test('the double process prints the ready line orcad prints and ends when its stdin closes', async () => {
  const child = spawn(process.execPath, [join(packageDir, 'src/orca/host-double.ts')], { env: { ...process.env, NODE_NO_WARNINGS: '1' }, stdio: ['pipe', 'pipe', 'inherit'] })
  const target = await readyTarget(createInterface({ input: child.stdout }))
  assert.match(target.endpoint, /^ws:\/\/127\.0\.0\.1:\d+$/)
  const closed = new Promise((r) => child.on('close', r))
  child.stdin.end()
  await closed
})

