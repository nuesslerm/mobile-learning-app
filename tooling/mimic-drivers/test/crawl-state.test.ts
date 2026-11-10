// Given the committed crawl of an app, When its cases are tagged by whether they can change what the target answers next, Then a case that a
// recording would batch next to others never belongs to a row whose answer depends on its neighbours, and a scenario never shares a server.
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { test } from 'node:test'
import { repoRoot } from '../src/lock.ts'
import { API_DIR, SET_DIR } from '../src/scenarios/api.ts'

type Case = { id: string; row: string; tags: string[] }
const cases = (dir: string): Case[] => JSON.parse(readFileSync(join(repoRoot, dir, 'corpus.json'), 'utf8')).generators.flatMap((g: { cases: Case[] }) => g.cases)

test('Given the T3 crawl, When a row is a command, a revoke or a subscription, Then every one of its cases, rejected inputs included, runs in the serial write group', () => {
  const rows = cases(API_DIR.t3).filter((c) => /^command:|revoke|subscribe/i.test(c.row))
  assert.ok(rows.length > 0)
  for (const c of rows) assert.ok(c.tags.includes('state:write'), `${c.id} would be recorded beside others`)
})

for (const app of ['orca', 't3'] as const) {
  test(`Given the ${app} scenario set, When it is recorded, Then each scenario asks for a server of its own`, () => {
    const all = cases(SET_DIR[app])
    assert.ok(all.length > 0)
    for (const c of all) assert.ok(c.tags.includes('state:fresh'), `${c.id} would share a server`)
  })
}
