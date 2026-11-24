// Given a release whose `t3` prints a help screen and serves its descriptor, When the runtime is captured, Then the help of the binary and of
// every subcommand it lists and the descriptor of the server are written where the crawl reads them, and the crawl counts them as sources.
import assert from 'node:assert/strict'
import { chmodSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { captureRuntime } from '../src/crawl/runtime.ts'
import { helpCommands } from '../src/crawl/t3.ts'

const FAKE_T3 = `#!/usr/bin/env node
const args = process.argv.slice(2)
if (args[0] === 'serve') {
  const port = Number(args[args.indexOf('--port') + 1])
  require('node:http').createServer((req, res) => {
    res.setHeader('content-type', 'application/json')
    res.end(req.url === '/.well-known/t3/environment' ? JSON.stringify({ environmentId: 'e-1', version: '0.0.0' }) : '{}')
  }).listen(port, '127.0.0.1')
} else if (args[0] === 'auth') {
  console.log('Usage: t3 auth <command>\\n\\nCommands:\\n  pairing  Create pairing links')
} else {
  console.log('Usage: t3 [command]\\n\\nCommands:\\n  serve  Start the server\\n  auth   Manage access')
}
`

test('the help of the binary and of each subcommand and the descriptor of a started server are written for the crawl', async () => {
  const release = mkdtempSync(join(tmpdir(), 'fake-release-'))
  writeFileSync(join(release, 't3'), FAKE_T3)
  chmodSync(join(release, 't3'), 0o755)
  const out = join(mkdtempSync(join(tmpdir(), 'runtime-out-')), 'runtime')
  const { commands } = await captureRuntime(out, release)
  assert.deepEqual(commands, ['serve', 'auth'])
  assert.deepEqual(readdirSync(out).sort(), ['descriptor.json', 'help-auth.txt', 'help-serve.txt', 'help.txt'])
  assert.deepEqual(helpCommands(readFileSync(join(out, 'help-auth.txt'), 'utf8')), ['pairing'])
  assert.equal(JSON.parse(readFileSync(join(out, 'descriptor.json'), 'utf8')).environmentId, 'e-1')
})

test('a server that exits before it answers is an error naming the exit', async () => {
  const release = mkdtempSync(join(tmpdir(), 'fake-release-'))
  mkdirSync(release, { recursive: true })
  writeFileSync(join(release, 't3'), '#!/bin/sh\nexit 3\n')
  chmodSync(join(release, 't3'), 0o755)
  await assert.rejects(() => captureRuntime(join(mkdtempSync(join(tmpdir(), 'runtime-out-')), 'r'), release), /exited with 3/)
})
