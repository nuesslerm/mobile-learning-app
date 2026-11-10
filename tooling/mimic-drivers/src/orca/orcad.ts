// The pinned orcad as something a driver can run: the Provisioner. Build recipe (pin commit checked out at `checkout`, then
// `pnpm install --frozen-lockfile` and `pnpm run build:orcad`, the script the pin's own terminal smoke uses) and launch (the packaged
// Node runs `orcad-server.js --port <p> --json` with a throwaway ORCA_USER_DATA; its first `orca_server_ready` JSON line carries the
// pairing offer). Not run in the sandbox: the build needs the VM, and the Ports below are what the flow tests double.
import { execFileSync, spawn } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync } from 'node:fs'
import { createServer } from 'node:net'
import { createInterface } from 'node:readline'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { readPin } from '../orca-lib/pin.ts'
import type { Target } from './target.ts'
import { targetFromPairingUri } from './target.ts'

export type Command = { program: string; args: string[]; cwd: string }

/** The steps that turn the checkout into a runnable orcad, as data (the VM runs them, on Node 24 as the pin's engines say; a double records them).
 * orcad stages the mobile web bundle, so that is built first. */
export const buildRecipe = (checkout: string): Command[] => [
  { program: 'pnpm', args: ['install', '--frozen-lockfile'], cwd: checkout },
  { program: 'pnpm', args: ['-C', 'mobile', 'install', '--frozen-lockfile'], cwd: checkout },
  { program: 'pnpm', args: ['run', 'build:mobile-web'], cwd: checkout },
  { program: 'pnpm', args: ['run', 'build:orcad'], cwd: checkout }
]

export type RunningOrcad = { stdout: AsyncIterable<string>; stop(): Promise<void> }

/** What provisioning needs from the machine: every one a double in the flow tests. */
export type Ports = {
  head(checkout: string): string
  run(command: Command): Promise<number>
  built(checkout: string): { node: string; entry: string } | null
  start(node: string, args: string[], env: Record<string, string>): RunningOrcad
  port(): Promise<number>
}

export type OrcadLaunch = <T>(use: (target: Target) => Promise<T>) => Promise<T>

export type ReadyLine = { type: string; pairing?: { url?: string } }

/** The target in the first ready line orcad prints; other lines (logs) are skipped. */
export async function readyTarget(lines: AsyncIterable<string>): Promise<Target> {
  for await (const line of lines) {
    if (!line.startsWith('{')) continue
    let parsed: ReadyLine
    try {
      parsed = JSON.parse(line) as ReadyLine
    } catch {
      continue
    }
    if (parsed.type !== 'orca_server_ready') continue
    if (!parsed.pairing?.url) throw new Error('the orcad ready line carried no pairing offer')
    return targetFromPairingUri(parsed.pairing.url)
  }
  throw new Error('orcad exited before it printed a ready line')
}

export function orcadLauncher(checkout: string, ports: Ports): OrcadLaunch {
  return async (use) => {
    const pin = readPin()
    const head = ports.head(checkout)
    if (head !== pin.commit) throw new Error(`${checkout} is at ${head}, the pin names ${pin.commit}`)
    if (!ports.built(checkout)) {
      for (const command of buildRecipe(checkout)) {
        const code = await ports.run(command)
        if (code !== 0) throw new Error(`${command.program} ${command.args.join(' ')} exited ${code}`)
      }
    }
    const built = ports.built(checkout)
    if (!built) throw new Error('the build recipe ran but left no out/orcad package')
    const userData = mkdtempSync(join(tmpdir(), 'mimic-orcad-'))
    // a home of its own: accounts, skills and AI sessions are discovered under HOME, and a shared /tmp made them differ run to run
    const home = join(userData, 'home')
    mkdirSync(home)
    const running = ports.start(built.node, [built.entry, '--port', String(await ports.port()), '--json'], { ORCA_USER_DATA: userData, HOME: home, XDG_CONFIG_HOME: join(home, '.config'), XDG_DATA_HOME: join(home, '.local', 'share'), TZ: 'UTC', LANG: 'C.UTF-8' })
    try {
      return await use(await readyTarget(running.stdout))
    } finally {
      await running.stop()
      rmSync(userData, { recursive: true, force: true })
    }
  }
}

export const gitHead = (checkout: string) => {
  const env = Object.fromEntries(Object.entries(process.env).filter(([k]) => !k.startsWith('GIT_')))
  return execFileSync('git', ['-C', checkout, 'rev-parse', 'HEAD'], { encoding: 'utf8', env }).trim()
}

/** The packaged Node and entry the build leaves (`out/orcad/orcad-server.js`, `out/runtimes/node-<sha256>/bin/node`), or null before a build. */
export function builtOrcad(checkout: string): { node: string; entry: string } | null {
  const entry = join(checkout, 'out', 'orcad', 'orcad-server.js')
  const runtimes = join(checkout, 'out', 'runtimes')
  if (!existsSync(entry) || !existsSync(runtimes)) return null
  const slot = readdirSync(runtimes).find((d) => d.startsWith('node-') && existsSync(join(runtimes, d, 'bin', 'node')))
  return slot ? { node: join(runtimes, slot, 'bin', 'node'), entry } : null
}

/** `MIMIC_FIXED_PORT` pins the port of every server (one at a time), so a port never reaches an answer; unset, a free one is asked for. */
const freePort = () =>
  process.env.MIMIC_FIXED_PORT ? Promise.resolve(Number(process.env.MIMIC_FIXED_PORT)) : new Promise<number>((resolve, reject) => {
    const s = createServer()
    s.listen(0, '127.0.0.1', () => {
      const { port } = s.address() as { port: number }
      s.close(() => resolve(port))
    })
    s.on('error', reject)
  })

/** The real machine: git, pnpm and the packaged Node as child processes. */
export const machinePorts: Ports = {
  head: gitHead,
  run: (c) =>
    new Promise((resolve, reject) => {
      const child = spawn(c.program, c.args, { cwd: c.cwd, stdio: ['ignore', 2, 2] })
      child.on('error', reject)
      child.on('close', (code) => resolve(code ?? 1))
    }),
  built: builtOrcad,
  port: freePort,
  start(node, args, env) {
    const child = spawn(node, args, { env: { PATH: process.env.PATH ?? '', HOME: tmpdir(), ...env }, stdio: ['ignore', 'pipe', 2] })
    return {
      stdout: createInterface({ input: child.stdout! }),
      stop: () =>
        new Promise<void>((resolve) => {
          if (child.exitCode !== null) return resolve()
          child.once('close', () => resolve())
          child.kill('SIGTERM')
        })
    }
  }
}
