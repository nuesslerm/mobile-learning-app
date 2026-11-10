// Given the committed crawl of an app, When its stateful scenarios are derived, Then the committed scenario set is what the generator makes, every
// endpoint is either asked with its entity present or listed with the reason it is not, a method whose payload names an entity always gets a
// scenario, a crawl that lacks what a scenario needs fails naming it, and the requests of a scenario carry what the earlier answers returned.
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { test } from 'node:test'
import { repoRoot } from '../src/lock.ts'
import { startHostDouble } from '../src/orca/host-double.ts'
import { runScript } from '../src/orca/client.ts'
import { API_DIR, SET_DIR, loadApi } from '../src/scenarios/api.ts'
import { render, scenarios } from '../src/scenarios/index.ts'
import { orcaScenarios } from '../src/scenarios/orca.ts'
import { t3Scenarios } from '../src/scenarios/t3.ts'

const APPS = ['orca', 't3'] as const
const read = (app: (typeof APPS)[number], file: string) => readFileSync(join(repoRoot, SET_DIR[app], file), 'utf8')

for (const app of APPS) {
  test(`Given the committed ${app} crawl, When the scenarios are derived, Then the committed set is exactly that, and every endpoint is accounted for once`, () => {
    const set = scenarios(app)
    const out = render(app, set)
    assert.equal(read(app, 'corpus.json'), out.corpus, `${SET_DIR[app]}/corpus.json is stale: node src/cli.ts scenarios ${app}`)
    assert.equal(read(app, 'scenarios.json'), out.scenarios, `${SET_DIR[app]}/scenarios.json is stale`)
    const api = loadApi(app)
    assert.deepEqual(set.coverage.map((c) => c.row), api.endpoints.map((e) => e.id), 'one coverage entry per endpoint of the crawl, in order')
    const ids = new Set(set.cases.map((c) => c.id))
    assert.equal(ids.size, set.cases.length, 'case ids are unique')
    assert.deepEqual([...ids].sort(), set.scenarios.map((s) => s.id).sort(), 'every scenario is exactly one case')
    const rows = new Set(api.endpoints.map((e) => e.id))
    for (const c of set.cases) assert.ok(rows.has(c.row), `${c.id} names an endpoint of the crawl`)
    for (const c of set.coverage) {
      if (c.state === 'populated') assert.ok(c.via.length > 0 && c.via.every((v) => ids.has(v)), `${c.row}: populated by a scenario that exists`)
      else assert.ok(c.reason && c.via.length === 0, `${c.row}: empty-only carries its reason`)
    }
  })
}

const named = (payload: unknown, keys: string[]) => payload !== null && typeof payload === 'object' && keys.some((k) => (payload as Record<string, unknown>)[k] === 'a')

test('Given the crawl, When a method names a repo, worktree or terminal (Orca) or a thread, project or cwd (T3), Then it is asked with that entity present, never left empty-only', () => {
  const cases = [
    { app: 'orca' as const, keys: ['repo', 'worktree', 'terminal'], skip: (e: { streaming: boolean; skipped?: string; transport: string }) => e.streaming || !!e.skipped || e.transport === 'http' },
    { app: 't3' as const, keys: ['threadId', 'projectId', 'parentThreadId', 'targetThreadId', 'cwd'], skip: (e: { skipped?: string }) => !!e.skipped }
  ]
  for (const { app, keys, skip } of cases) {
    const api = loadApi(app)
    const covered = new Map(scenarios(app).coverage.map((c) => [c.row, c]))
    const expected = api.endpoints.filter((e) => !skip(e) && named(api.payloads.get(e.id)?.valid, keys))
    assert.ok(expected.length > 50, `${app}: the crawl has many entity-naming methods`)
    for (const e of expected) assert.equal(covered.get(e.id)?.state, 'populated', `${app} ${e.id} names an entity and has no populated scenario`)
  }
})

test('Given a crawl that lacks what a scenario needs, When the scenarios are derived, Then it fails naming the missing provider or key instead of writing a hollow scenario', () => {
  const orca = loadApi('orca')
  const noProvider = { ...orca, payloads: new Map([...orca.payloads].filter(([row]) => row !== 'rpc:repo.add')) }
  assert.throws(() => orcaScenarios(noProvider), /repo\.add has no valid payload/)
  const noKey = { ...orca, payloads: new Map(orca.payloads).set('rpc:files.write', { valid: { worktree: 'a' } }) }
  assert.throws(() => orcaScenarios(noKey), /files\.write: the crawl's payloads have no key (content|relativePath)/)
  const t3 = loadApi('t3')
  const noTurn = { ...t3, payloads: new Map([...t3.payloads].filter(([row]) => row !== 'command:message.dispatch')) }
  assert.throws(() => t3Scenarios(noTurn), /message\.dispatch/)
})

test('Given an Orca scenario for a file method and a host that answers the creations, When the scenario runs, Then the file request names the worktree the host returned and a file the repository has', async () => {
  const set = scenarios('orca')
  const scenario = set.cases.find((c) => c.id === 'orca-scn-files.read')!
  const double = await startHostDouble({ methods: { 'repo.add': { result: { repo: { id: 'repo-9' } } }, 'worktree.create': { result: { worktree: { id: 'wt-9' } } }, 'files.read': { result: { content: '# mimic' } } } })
  try {
    const reply = await runScript(double, scenario.input as never)
    const sent = double.calls.filter((c) => !c.method.startsWith('runtime.'))
    assert.deepEqual(sent.map((c) => c.method), ['repo.add', 'worktree.create', 'files.read'])
    assert.deepEqual(sent[1]!.params, { repo: 'id:repo-9' })
    assert.deepEqual(sent[2]!.params, { worktree: 'id:wt-9', relativePath: 'README.md' })
    assert.equal(reply.steps.filter((s) => s.dir === 's2c').every((s) => (s.payload as { ok?: boolean }).ok === true), true)
  } finally {
    await double.close()
  }
})

test('Given the API directories the scenarios derive from, When the layout is read, Then each scenario set sits beside its crawl', () => {
  for (const app of APPS) assert.equal(join(SET_DIR[app], '..'), join(API_DIR[app], '..'))
})
