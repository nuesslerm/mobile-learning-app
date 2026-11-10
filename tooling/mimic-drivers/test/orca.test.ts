// Given the Orca ws driver, a scripted double of an Orca host (the pinned desktop e2ee channel behind a WebSocket) and a fake orcad machine,
// When the driver runs request scripts on the phone's pinned transport and the provisioner builds and launches an orcad at the pin,
// Then the driver reports the decrypted request and response frames, a wrong credential or a dead endpoint as an error outcome, and the
// provisioner refuses a checkout off the pin, runs the build recipe once, and pairs through the ready line.
import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { existsSync } from 'node:fs'
import { createInterface } from 'node:readline'
import { join } from 'node:path'
import { test } from 'node:test'
import { packageDir, toolchainLockHash } from '../src/lock.ts'
import { runScript } from '../src/orca/client.ts'
import { startHostDouble, type Methods } from '../src/orca/host-double.ts'
import { buildRecipe, orcadLauncher, readyTarget, type Command, type Ports } from '../src/orca/orcad.ts'
import { orca } from '../src/orca/index.ts'
import { targetFromEnv } from '../src/orca/target.ts'
import { readPin } from '../src/orca-lib/pin.ts'
import { answer } from '../src/protocol.ts'

const METHODS: Methods = {
  'files.list': { result: { entries: [{ name: 'a.txt', kind: 'file' }] } },
  'files.read': { error: { code: 'not_found', message: 'no such file' } }
}

test('a script is answered frame by frame: requests as sent, responses as the host gave them, minus the per-run id', async () => {
  const double = await startHostDouble({ methods: METHODS })
  try {
    const reply = await runScript(double, {
      requests: [{ method: 'files.list', params: { path: '.' } }, { method: 'files.read', params: { path: 'x' } }, { method: 'nope' }]
    })
    assert.equal(reply.outcome?.status, 'ok')
    const responses = reply.steps.filter((s) => s.dir === 's2c').map((s) => s.payload as any)
    assert.deepEqual(responses[0].result, (METHODS['files.list'] as any).result)
    assert.equal(responses[1].ok, false)
    assert.equal(responses[1].error.code, (METHODS['files.read'] as any).error.code)
    assert.equal(responses[2].error.code, 'method_not_found')
    assert.ok(reply.steps.every((s) => !('id' in (s.payload as object))), 'no minted id leaks into the steps')
    assert.deepEqual(reply.steps.filter((s) => s.dir === 'c2s').map((s) => (s.payload as any).method), ['files.list', 'files.read', 'nope'])
    assert.deepEqual(double.asked.filter((m) => !m.startsWith('runtime.')), ['files.list', 'files.read', 'nope'])
  } finally {
    await double.close()
  }
})

test('a double answers by the params a request carries when its method lists several answers', async () => {
  // Given a method with an answer per path and none for the rest
  const methods: Methods = {
    'files.stat': [
      { when: { relativePath: 'a.txt' }, result: { size: 3 } },
      { when: { relativePath: 'dir' }, result: { size: 0, isDirectory: true } }
    ]
  }
  const double = await startHostDouble({ methods })
  try {
    // When the phone asks about each path and one that is not listed
    const reply = await runScript(double, { requests: ['dir', 'a.txt', 'zzz'].map((relativePath) => ({ method: 'files.stat', params: { relativePath } })) })
    // Then each gets its own answer and the unlisted one is refused like an unknown method
    const answers = reply.steps.filter((s) => s.dir === 's2c').map((s) => s.payload as any)
    assert.deepEqual([answers[0].result, answers[1].result], [{ size: 0, isDirectory: true }, { size: 3 }])
    assert.equal(answers[2].error.code, 'method_not_found')
  } finally {
    await double.close()
  }
})

test('a token the host does not know ends the run as an error outcome naming the client state, with no steps', async () => {
  const double = await startHostDouble({ methods: METHODS })
  try {
    const reply = await runScript({ ...double, deviceToken: 'not-the-paired-token' }, { requests: [{ method: 'files.list' }], connect_timeout_ms: 20_000 })
    assert.equal(reply.outcome?.status, 'error')
    assert.equal(reply.outcome?.code, 'auth-failed')
    assert.deepEqual(reply.steps, [])
    assert.deepEqual(double.asked, [])
  } finally {
    await double.close()
  }
})

test('a host that never answers ends the run as a timeout outcome', async () => {
  const double = await startHostDouble({ methods: METHODS })
  const dead = { ...double, endpoint: 'ws://127.0.0.1:1' }
  try {
    const reply = await runScript(dead, { requests: [], connect_timeout_ms: 1500 })
    assert.equal(reply.outcome?.status, 'error')
    assert.deepEqual(reply.steps, [])
    assert.notEqual(reply.outcome?.code, 'connected')
  } finally {
    await double.close()
  }
})

test('the driver runs through mimic-driver/1 against the host the environment names', async () => {
  const double = await startHostDouble({ methods: METHODS })
  const env = { ...process.env, ORCA_ENDPOINT: double.endpoint, ORCA_DEVICE_TOKEN: double.deviceToken, ORCA_SERVER_PUBLIC_KEY_B64: double.serverPublicKeyB64 }
  const keep = Object.fromEntries(Object.keys(env).map((k) => [k, process.env[k]]))
  Object.assign(process.env, env)
  try {
    const driver = orca()
    const hello: any = await answer(driver, '{"hello":true}')
    assert.equal(hello.hello.driver.toolchain_lock_hash, toolchainLockHash())
    assert.equal(hello.hello.target_version, readPin().version)
    const reply: any = await answer(driver, JSON.stringify({ id: '1', op: 'script', input: { requests: [{ method: 'files.list' }] } }))
    assert.equal(reply.outcome.status, 'ok')
    assert.equal(reply.steps[1].payload.ok, true)
  } finally {
    for (const [k, v] of Object.entries(keep)) {
      if (v === undefined) delete process.env[k]
      else process.env[k] = v
    }
    await double.close()
  }
})

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
  const reply = await launch((target) => runScript(target, { requests: [{ method: 'files.list' }] }))
  assert.equal(reply.outcome?.status, 'ok')
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

test('Given requests that name what an earlier answer returned, When the script runs, Then the captured value (with its prefix) is sent, a repository made by setup is real and masked, and an unbound value is not sent', async () => {
  const double = await startHostDouble({
    methods: {
      'repo.add': { result: { repo: { id: 'r-1' } } },
      'worktree.create': { result: { nothing: true } },
      'repo.show': { result: {} }
    }
  })
  try {
    const reply = await runScript(double, {
      setup: [{ git: 'main' }],
      requests: [
        { method: 'repo.add', params: { path: '{git.main}' }, capture: { repo: { from: ['/result/repo/id'], prefix: 'id:' } } },
        { method: 'worktree.create', params: { repo: '{repo}' }, capture: { worktree: { from: ['/result/worktree/id'], prefix: 'id:' } } },
        { method: 'repo.show', params: { repo: '{repo}', worktree: '{worktree}' } },
        { method: 'repo.show', params: { repo: '{repo}' } }
      ]
    })
    const sent = double.calls.filter((c) => !c.method.startsWith('runtime.'))
    const added = sent[0]!.params as { path: string }
    assert.ok(existsSync(join(added.path, '.git')) === false, 'the scratch repository is removed when the script ends')
    assert.match(added.path, /\/main$/)
    assert.deepEqual(sent.slice(1).map((c) => c.params), [{ repo: 'id:r-1' }, { repo: 'id:r-1' }], 'the captured id reaches the next request; the request with an unbound worktree was not sent')
    const answers = reply.steps.filter((s) => s.dir === 's2c').map((s) => s.payload as { client_error?: string })
    assert.equal(answers[2]!.client_error, 'unbound variable worktree')
    assert.equal(JSON.stringify(reply.steps).includes(added.path), false, 'the per-run scratch path does not reach the recorded steps')
    assert.match(JSON.stringify(reply.steps[0]), /<scratch>\/main/)
  } finally {
    await double.close()
  }
})
