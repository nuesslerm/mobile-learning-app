// The T3 driver: replays the client side of an interaction (its `script`, see client.ts) against the pinned T3 server and reports the wire.
// By default it starts the release the pin names (release.ts) and keeps one server for many interactions (the start costs seconds): the
// session prelude (pairing, socket, project, thread) runs once per server and its steps open every interaction that asks for it, and a `reset`
// request starts a new server. `MIMIC_SESSION=fresh` is the old behaviour, one fresh server per run. `T3_URL` and `T3_PAIRING_CREDENTIAL`
// point it at a server that is already running instead (one credential, so one run); that is how a candidate is driven by hand.
import { homedir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { goldenPayloads, prelude } from '../corpus/t3.ts'
import type { Driver, Step } from '../protocol.ts'
import { shared } from '../session.ts'
import { runScript, withoutWorkspace, type Script } from './client.ts'
import { ensureRuntime, loadPin, pinnedRelease, type Launch, type Running } from './release.ts'

const PIN = fileURLToPath(new URL('../../../t3-conformance/t3-pin.json', import.meta.url))

function launcher(env: NodeJS.ProcessEnv): Launch {
  if (env.T3_URL) {
    const credential = env.T3_PAIRING_CREDENTIAL
    if (!credential) throw new Error('T3_URL needs T3_PAIRING_CREDENTIAL')
    return (use) => use({ baseUrl: env.T3_URL!, workspace: env.T3_WORKSPACE ?? '/conformance/workspace', mint: async () => credential })
  }
  const pin = loadPin(PIN)
  const platform = `linux-${process.arch === 'arm64' ? 'arm64' : 'x64'}`
  const cache = env.T3_RELEASE_CACHE ?? join(homedir(), '.cache', 'mimic-t3')
  let runtime: ReturnType<typeof ensureRuntime> | undefined
  return (use) => {
    runtime ??= ensureRuntime({ pin, platform, cache })
    return runtime.then((r) => pinnedRelease(r)(use))
  }
}

type Case = { script: Script; settle_ms?: number; prelude?: 'session' }
type Prelude = { generation: number; ready: Promise<{ steps: Step[]; accessToken: string }> }

const staticVars = (server: Running): Record<string, string> => ({ workspace: server.workspace, 'bad-credential': 'NOTACREDENTIAL' })

async function finish(server: Running, steps: Step[]) {
  const cut = steps.at(-1)?.channel === 'driver' && 'transport_error' in (steps.at(-1)!.payload as object)
  return { steps: withoutWorkspace(steps, server.workspace), effects: [], outcome: cut ? ({ status: 'error', code: 'transport' } as const) : ({ status: 'ok' } as const) }
}

/** The pairing, socket, project and thread of the server in the state it starts in: what every interaction of its session begins with. */
async function preludeOf(server: Running): Promise<{ steps: Step[]; accessToken: string }> {
  const vars: Record<string, string> = { ...staticVars(server), credential: await server.mint() }
  const steps = await runScript(server.baseUrl, [...prelude(goldenPayloads()), { 'ws-close': {} }], vars, 250)
  const closed = steps.findIndex((s) => s.channel === 'ws:close')
  return { steps: closed < 0 ? steps : steps.slice(0, closed), accessToken: vars.access_token ?? '' }
}

export function t3(launch?: Launch, version = loadPin(PIN).version, opts: { session?: boolean } = {}): Driver {
  const session = opts.session ?? (launch === undefined && process.env.MIMIC_SESSION !== 'fresh')
  const start = launch ?? launcher(process.env)
  const held = session ? shared<Running>(start) : undefined
  let pre: Prelude | undefined

  // one run on its own server: the prelude is part of the script, nothing is shared
  const alone = (input: Case) =>
    start(async (server) => {
      const script: Script = input.prelude === 'session' ? [...prelude(goldenPayloads()), ...input.script] : input.script
      const vars = staticVars(server)
      if (JSON.stringify(script).includes('{credential}')) vars.credential = await server.mint()
      return finish(server, await runScript(server.baseUrl, script, vars, input.settle_ms ?? 250))
    })

  const together = async (input: Case) => {
    const { server, generation } = await held!.get()
    const vars = staticVars(server)
    if (input.prelude !== 'session') {
      if (JSON.stringify(input.script).includes('{credential}')) vars.credential = await server.mint()
      return finish(server, await runScript(server.baseUrl, input.script, vars, input.settle_ms ?? 250))
    }
    if (pre?.generation !== generation) pre = { generation, ready: preludeOf(server) }
    const begun = await pre.ready
    vars.access_token = begun.accessToken
    // a socket of its own on the server's project and thread; its opening steps are the prelude's, copied
    const own: Script = [prelude(goldenPayloads())[1]!, prelude(goldenPayloads())[2]!]
    const steps = await runScript(server.baseUrl, [...own, ...input.script], vars, input.settle_ms ?? 250)
    const opened = steps.findIndex((s) => s.channel === 'ws:open')
    return finish(server, [...begun.steps, ...(opened < 0 ? steps : steps.slice(opened + 1))])
  }

  return {
    name: 't3',
    version: '0.1.0',
    protocol: 'mimic-driver/1',
    targetVersion: version,
    ops: {
      // `prelude: "session"` is the pairing, the socket and the project and thread every row that names one needs, kept out of each case
      script: (input: Case) => (held ? together(input) : alone(input)),
    },
    reset: async () => {
      await held?.reset()
      pre = undefined
    },
    close: async () => {
      await held?.close()
    },
  }
}
