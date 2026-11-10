// The pinned T3 release as something a driver can run: the Provisioner. It fetches the release tarball the pin names, refuses any bytes
// whose sha256 the pin and the release's SHA256SUMS do not both give, unpacks it into a throwaway T3 home and starts the server on loopback
// in an empty environment (an inherited one leaked local Claude configuration into the provider list, docs/plans/t3-parity/phase0-report.md).
import { spawn, spawnSync, type ChildProcess } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs'
import { createServer } from 'node:net'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'

export type PlatformPin = { bytes: number; sha256: string }
export type Pin = { version: string; commit: string; release: string; platforms: Record<string, PlatformPin> }
export type Get = (url: string) => Promise<Buffer>

export const loadPin = (path: string): Pin => JSON.parse(readFileSync(path, 'utf8')) as Pin
export const assetName = (version: string, platform: string) => `t3-${version}-${platform}.tar.gz`
export const sha256 = (b: Buffer) => createHash('sha256').update(b).digest('hex')

export function parseSha256Sums(text: string): Map<string, string> {
  const out = new Map<string, string>()
  for (const line of text.split('\n')) {
    const m = /^([0-9a-f]{64})\s+\*?(\S+)$/.exec(line.trim())
    if (m) out.set(m[2]!, m[1]!)
  }
  return out
}

// Node's fetch ignores HTTPS_PROXY unless NODE_USE_ENV_PROXY=1 is set in the environment.
export const httpGet: Get = async (url) => {
  const res = await fetch(url, { redirect: 'follow' })
  if (!res.ok) throw new Error(`GET ${url}: ${res.status}`)
  return Buffer.from(await res.arrayBuffer())
}

export type Runtime = { dir: string; tarball: string; downloaded: boolean }

/** The verified runtime of the pin under `cache`: the tarball is fetched once, and unpacked once per `cache`. */
export async function ensureRuntime(o: { pin: Pin; platform: string; cache: string; get?: Get }): Promise<Runtime> {
  const want = o.pin.platforms[o.platform]
  if (!want) throw new Error(`the pin has no platform ${o.platform}`)
  const get = o.get ?? httpGet
  const name = assetName(o.pin.version, o.platform)
  const tarball = join(o.cache, name)
  let downloaded = false
  if (!(existsSync(tarball) && sha256(readFileSync(tarball)) === want.sha256)) {
    rmSync(tarball, { force: true })
    const sums = parseSha256Sums((await get(`${o.pin.release}/SHA256SUMS`)).toString('utf8'))
    if (sums.get(name) !== want.sha256) throw new Error(`SHA256SUMS of the release disagrees with the pin for ${name}`)
    const bytes = await get(`${o.pin.release}/${name}`)
    if (sha256(bytes) !== want.sha256) throw new Error(`sha256 of ${name} does not match the pin`)
    mkdirSync(o.cache, { recursive: true })
    writeFileSync(tarball, bytes)
    downloaded = true
  }
  const dir = join(o.cache, `runtime-${o.pin.version}-${o.platform}`)
  if (!existsSync(dir)) {
    const tmp = mkdtempSync(join(dirname(dir), '.extract-'))
    try {
      const r = spawnSync('tar', ['xzf', tarball, '-C', tmp, '--strip-components=1'])
      if (r.status !== 0) throw new Error(`tar failed: ${r.stderr?.toString() ?? ''}`)
      renameSync(tmp, dir)
    } finally {
      rmSync(tmp, { recursive: true, force: true })
    }
  }
  return { dir, tarball, downloaded }
}

/** The credential in what `t3 auth pairing create --json` printed: its first string among the names a pairing code goes by. */
export function credentialOf(stdout: string): string {
  const doc = JSON.parse(stdout) as Record<string, unknown>
  for (const key of ['credential', 'token', 'code', 'secret']) if (typeof doc[key] === 'string') return doc[key]
  throw new Error(`the pairing command printed no credential (keys: ${Object.keys(doc).join(', ')})`)
}

export type Running = { baseUrl: string; workspace: string; mint: () => Promise<string> }
/** Runs `use` against a server that is started for it and stopped after. */
export type Launch = <T>(use: (server: Running) => Promise<T>) => Promise<T>

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

async function ready(base: string, child: ChildProcess, ms: number): Promise<void> {
  const until = Date.now() + ms
  while (Date.now() < until) {
    if (child.exitCode !== null) throw new Error(`the server exited with ${child.exitCode} before it served`)
    try {
      if ((await fetch(`${base}/.well-known/t3/environment`)).ok) return
    } catch {
      // not listening yet
    }
    await new Promise((r) => setTimeout(r, 250))
  }
  throw new Error(`the server did not serve ${base} within ${ms} ms`)
}

/** A fresh server of the pinned release per use (a new home and workspace each time, so a project made by one run is not there for the next). */
export function pinnedRelease(runtime: Runtime, readyMs = 60_000): Launch {
  return async (use) => {
    const home = mkdtempSync(join(tmpdir(), 't3-home-'))
    const workspace = mkdtempSync(join(tmpdir(), 't3-workspace-'))
    // T3_CLI_DIR holds the scripted `claude`/`codex` (exec-testkit): a turn runs it, no model credentials involved
    const env = { PATH: [process.env.T3_CLI_DIR, '/usr/bin:/bin'].filter(Boolean).join(':'), HOME: home, T3CODE_HOME: home, TZ: 'UTC', LANG: 'C.UTF-8' }
    const binary = join(runtime.dir, 't3')
    const port = await freePort()
    const baseUrl = `http://127.0.0.1:${port}`
    const child = spawn(binary, ['serve', '--host', '127.0.0.1', '--port', String(port), '--no-browser', workspace], { env, stdio: 'ignore' })
    try {
      await ready(baseUrl, child, readyMs)
      const mint = async () => {
        const r = spawnSync(binary, ['auth', 'pairing', 'create', '--json'], { env })
        if (r.status !== 0) throw new Error(`pairing create failed: ${r.stderr?.toString() ?? ''}`)
        return credentialOf(r.stdout.toString())
      }
      return await use({ baseUrl, workspace, mint })
    } finally {
      // the server writes into its home until it is gone: removing it first fails with ENOTEMPTY
      const gone = child.exitCode !== null ? Promise.resolve() : new Promise<void>((r) => child.once('close', () => r()))
      child.kill('SIGTERM')
      await gone
      rmSync(home, { recursive: true, force: true, maxRetries: 5 })
      rmSync(workspace, { recursive: true, force: true, maxRetries: 5 })
    }
  }
}
