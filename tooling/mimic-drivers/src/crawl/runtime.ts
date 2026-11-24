// What the pinned T3 release says about itself, written where `crawl t3 --runtime <dir>` reads it: `help.txt` and `help-<command>.txt` (the
// release binary's `--help` of every subcommand) and `descriptor.json` (`/.well-known/t3/environment` of a server it starts). The release comes
// from the cache the recorder shares (T3_RELEASE_CACHE), downloaded and unpacked the way the recorder does when it is not there.
import { execFileSync, spawn } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs'
import { createServer } from 'node:net'
import { homedir, tmpdir } from 'node:os'
import { join } from 'node:path'
import { repoRoot } from '../lock.ts'
import { helpCommands } from './t3.ts'

type Pin = { version: string; release: string; platforms: Record<string, { sha256: string }> }

const pinFile = () => process.env.MIMIC_T3_PIN || join(repoRoot, 'tooling', 't3-conformance', 't3-pin.json')
const platformOf = () => `linux-${process.arch === 'arm64' ? 'arm64' : 'x64'}`
const cacheDir = () => process.env.T3_RELEASE_CACHE || join(homedir(), '.cache', 'mimic-t3')
const sha256 = (bytes: Buffer) => createHash('sha256').update(bytes).digest('hex')

/** The unpacked release (`t3` inside), fetched and verified against the pin when the cache lacks it. */
export function ensureRelease(pin: Pin = JSON.parse(readFileSync(pinFile(), 'utf8')), platform = platformOf(), cache = cacheDir()): string {
  const dir = join(cache, `runtime-${pin.version}-${platform}`)
  if (existsSync(join(dir, 't3'))) return dir
  const want = pin.platforms[platform]?.sha256
  if (!want) throw new Error(`the pin has no platform ${platform}`)
  mkdirSync(cache, { recursive: true })
  const tarball = join(cache, `t3-${pin.version}-${platform}.tar.gz`)
  if (!existsSync(tarball) || sha256(readFileSync(tarball)) !== want) {
    writeFileSync(tarball, execFileSync('curl', ['-fsSL', '--retry', '3', `${pin.release}/t3-${pin.version}-${platform}.tar.gz`], { maxBuffer: 1 << 30 }))
    if (sha256(readFileSync(tarball)) !== want) throw new Error(`sha256 of ${tarball} does not match the pin`)
  }
  const scratch = mkdtempSync(join(cache, '.extract-'))
  execFileSync('tar', ['xzf', tarball, '-C', scratch, '--strip-components=1'])
  rmSync(dir, { recursive: true, force: true })
  renameSync(scratch, dir)
  return dir
}

function helpOf(binary: string, args: string[]): string {
  try {
    return execFileSync(binary, [...args, '--help'], { encoding: 'utf8', env: { PATH: process.env.PATH ?? '', HOME: tmpdir(), NO_COLOR: '1' }, stdio: ['ignore', 'pipe', 'pipe'] })
  } catch (e) {
    // a help screen may exit non-zero: what it printed is still the help
    const out = e as { stdout?: string; stderr?: string }
    return `${out.stdout ?? ''}${out.stderr ?? ''}`
  }
}

const freePort = () =>
  new Promise<number>((resolve, reject) => {
    const s = createServer()
    s.once('error', reject)
    s.listen(0, '127.0.0.1', () => {
      const { port } = s.address() as { port: number }
      s.close(() => resolve(port))
    })
  })

async function descriptorOf(binary: string): Promise<unknown> {
  const home = mkdtempSync(join(tmpdir(), 'mimic-capture-'))
  const workspace = join(home, 'workspace')
  mkdirSync(workspace)
  const port = await freePort()
  const child = spawn(binary, ['serve', '--host', '127.0.0.1', '--port', String(port), '--no-browser', workspace], { env: { PATH: process.env.PATH ?? '', HOME: home, TZ: 'UTC' }, stdio: 'ignore' })
  try {
    const deadline = Date.now() + 120_000
    for (;;) {
      try {
        const r = await fetch(`http://127.0.0.1:${port}/.well-known/t3/environment`)
        if (r.ok) return await r.json()
      } catch {
        // not listening yet
      }
      if (child.exitCode !== null) throw new Error(`the server exited with ${child.exitCode} before it answered`)
      if (Date.now() > deadline) throw new Error('the server did not answer /.well-known/t3/environment in 120 s')
      await new Promise((r) => setTimeout(r, 250))
    }
  } finally {
    child.kill()
    rmSync(home, { recursive: true, force: true })
  }
}

export async function captureRuntime(out: string, binaryDir = ensureRelease()): Promise<{ commands: string[] }> {
  const binary = join(binaryDir, 't3')
  mkdirSync(out, { recursive: true })
  const top = helpOf(binary, [])
  writeFileSync(join(out, 'help.txt'), top)
  const commands = [...new Set(helpCommands(top))]
  for (const name of commands) writeFileSync(join(out, `help-${name}.txt`), helpOf(binary, [name]))
  writeFileSync(join(out, 'descriptor.json'), `${JSON.stringify(await descriptorOf(binary), null, 2)}\n`)
  return { commands }
}
