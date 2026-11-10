// Given the orca-lib driver as a child process speaking mimic-driver/1,
// When it is asked what it is, asked for things it cannot do, or pointed at Orca code that is not the pin,
// Then it names the pinned toolchain, answers a bad line with an error and keeps serving, and refuses the wrong commit by name.
import assert from 'node:assert/strict'
import { execFileSync, spawn } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { packageDir, repoRoot, toolchainLockHash } from '../src/lock.ts'

function talk(lines: string[], env: Record<string, string> = {}): Promise<any[]> {
  return new Promise((resolve, reject) => {
    const child = spawn('node', [join(packageDir, 'src/cli.ts'), 'serve', 'orca-lib'], { env: { ...process.env, NODE_NO_WARNINGS: '1', ...env } })
    let out = ''
    let err = ''
    child.stdout.on('data', (d) => (out += d))
    child.stderr.on('data', (d) => (err += d))
    child.on('close', () => (out.trim() ? resolve(out.trim().split('\n').map((l) => JSON.parse(l))) : reject(new Error(`no reply: ${err}`))))
    child.stdin.end(lines.join('\n') + '\n')
  })
}

test('hello names the driver, the pinned toolchain and the Orca version of the pin', async () => {
  const [reply] = await talk(['{"hello":true}'])
  assert.equal(reply.hello.driver.protocol, 'mimic-driver/1')
  assert.equal(reply.hello.driver.toolchain_lock_hash, toolchainLockHash())
  assert.match(reply.hello.target_version, /^\d+\.\d+\.\d+$/)
})

test('the toolchain hash moves with a pin of this package and with nothing else in the lockfile', () => {
  const lock = readFileSync(join(repoRoot, 'pnpm-lock.yaml'), 'utf8')
  const here = toolchainLockHash(lock)
  assert.notEqual(toolchainLockHash(lock.replace(/(  zod@4\.6\.7:\n    resolution: \{integrity: sha512-)/, '$1X')), here)
  assert.equal(toolchainLockHash(lock.replace(/(  tooling\/browser-harness:\n)/, '$1  # unrelated\n')), here)
})

test('a line that is not a request is answered with an error and the next request still runs', async () => {
  const replies = await talk(['not json', '{"id":"x","op":"nope","input":{}}', '{"id":"k","op":"crypto.fixed-bytes","input":{"fill":"ab","length":3}}'])
  assert.match(replies[0].error, /not JSON/)
  assert.match(replies[1].error, /unknown op nope/)
  assert.equal(replies[1].id, 'x')
  assert.equal(replies[2].steps[0].payload.hex, 'ababab')
})

test('Orca code that is not at the pinned commit is refused, naming both commits', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'not-the-pin-'))
  try {
    execFileSync('git', ['init', '-q', dir])
    execFileSync('git', ['-C', dir, '-c', 'user.name=t', '-c', 'user.email=t@t', 'commit', '-q', '--allow-empty', '-m', 'x'])
    const head = execFileSync('git', ['-C', dir, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim()
    const pin = join(dir, 'pin.json')
    writeFileSync(pin, JSON.stringify({ version: '9.9.9', commit: 'f'.repeat(40) }))
    const replies = await talk(['{"id":"m","op":"bundle.error-codes","input":{}}'], { MIMIC_ORCA_ROOT: dir, MIMIC_ORCA_PIN: pin })
    assert.match(replies[0].error, new RegExp(head))
    assert.match(replies[0].error, /f{40}/)
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})
