// Given the committed crawl of T3, When the agent-method scenarios are derived, Then the committed set is what the generator makes, each case ends with
// the row whose agent method it must show (the label of the case is its last row), the project cases make a project of their own and wait for the create
// before the delete, the branch rows are asked of a workspace that vcs.init made a repository, and fork and merge back send the decoder's latest_stable.
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { test } from 'node:test'
import { repoRoot } from '../src/lock.ts'
import { loadApi } from '../src/scenarios/api.ts'
import { AGENT_METHODS_DIR, agentMethods, agentTurns, render } from '../src/scenarios/index.ts'
import { t3AgentMethodScenarios } from '../src/scenarios/t3-agent.ts'

type Step = Record<string, unknown>
const scriptOf = (c: { input: unknown }) => (c.input as { script: Step[] }).script
const payloadOf = (s: Step) => (s['ws-send'] as { payload: Record<string, unknown> }).payload
const sends = (script: Step[]) => script.filter((s) => s['ws-send'])
const typeOf = (s: Step) => (payloadOf(s).type ?? (s['ws-send'] as { tag: string }).tag) as string
const cases = () => new Map(agentMethods().cases.map((c) => [c.id, c]))
const read = (file: string) => readFileSync(join(repoRoot, AGENT_METHODS_DIR, file), 'utf8')

test('Given the committed crawl, When the agent-method scenarios are derived, Then the committed set is exactly that', () => {
  const out = render('t3', agentMethods(), 't3-agent-methods')
  assert.equal(read('corpus.json'), out.corpus, 'corpus.json is stale: node src/cli.ts scenarios t3-methods')
  assert.equal(read('scenarios.json'), out.scenarios)
})

test('Given the two agent sets, When their case ids are read, Then no id is in both, so each interaction of the recordings is told apart', () => {
  const turns = new Set(agentTurns().cases.map((c) => c.id))
  for (const c of agentMethods().cases) assert.ok(!turns.has(c.id), c.id)
})

test('Given the cases, When one is labelled, Then its row is the last row it names, and queued-run.start is no longer labelled run.interrupt', () => {
  const rows = new Map([...cases()].map(([id, c]) => [id, c.row]))
  assert.deepEqual(Object.fromEntries(rows), {
    't3-method-queued-run.start': 'command:queued-run.start',
    't3-method-thread.fork': 'command:thread.fork',
    't3-method-thread.merge_back': 'command:thread.merge_back',
    't3-method-thread.create': 'command:thread.create',
    't3-method-project.create': 'mutation:project.create',
    't3-method-project.delete': 'mutation:project.delete',
    't3-method-vcs.refreshBranch': 'rpc:vcs.refreshBranch',
    't3-method-subscribeVcsBranch': 'rpc:subscribeVcsBranch'
  })
})

test('Given the queued-run.start case, When it is read, Then the queue is held by an interrupt of a hung turn before the queued run is started by hand', () => {
  const c = cases().get('t3-method-queued-run.start')!
  const script = scriptOf(c)
  assert.deepEqual(script[0]!['cli.script'], { cli: 'claude', turns: [{ hang: true }] })
  const order = sends(script).map(typeOf)
  assert.ok(order.indexOf('run.interrupt') < order.indexOf('queued-run.start'))
  assert.equal(payloadOf(sends(script).find((s) => typeOf(s) === 'run.interrupt')!).holdQueue, true)
})

test('Given a fork or a merge back, When its case is read, Then the source point is the latest_stable member and the fork is answered before the merge', () => {
  for (const id of ['t3-method-thread.fork', 't3-method-thread.merge_back']) {
    const moves = sends(scriptOf(cases().get(id)!)).filter((s) => ['thread.fork', 'thread.merge_back'].includes(typeOf(s)))
    assert.ok(moves.length > 0, id)
    for (const s of moves) assert.deepEqual(payloadOf(s).sourcePoint, { type: 'latest_stable' }, id)
  }
  const fork = sends(scriptOf(cases().get('t3-method-thread.merge_back')!)).find((s) => typeOf(s) === 'thread.fork')!
  assert.equal(fork.await, 'exit')
})

test('Given the thread.create case, When it is read, Then the thread is new, of Claude, in the prelude project, and the create is answered before the projection', () => {
  const s = sends(scriptOf(cases().get('t3-method-thread.create')!))
  assert.deepEqual(s.map(typeOf), ['thread.create', 'orchestration.getThreadProjection'])
  assert.deepEqual(payloadOf(s[0]!).modelSelection, { instanceId: 'claudeAgent', model: 'sonnet' })
  assert.equal(payloadOf(s[0]!).projectId, 'conformance-project')
  assert.notEqual(payloadOf(s[0]!).threadId, 'conformance-thread', 'the prelude holds that thread: a repeat is the recorded refusal, not a success')
  assert.equal(s[0]!.await, 'exit')
})

test('Given the project cases, When one is read, Then it makes a project of its own in a folder below the workspace, and the delete follows an answered create of it', () => {
  const create = payloadOf(sends(scriptOf(cases().get('t3-method-project.create')!))[0]!)
  assert.equal(create.type, 'project.create')
  assert.notEqual(create.projectId, 'conformance-project', 'the prelude holds that project: a repeat is the recorded refusal, not a success')
  assert.match(create.workspaceRoot as string, /^\{workspace\}\/.+/)
  assert.equal(create.createWorkspaceRootIfMissing, true)
  assert.ok(!scriptOf(cases().get('t3-method-project.create')!).some((s) => s['ws-send'] && typeOf(s) === 'thread.create'), 'no thread is made')
  const del = sends(scriptOf(cases().get('t3-method-project.delete')!))
  assert.deepEqual(del.map(typeOf), ['project.create', 'project.delete'])
  assert.equal(del[0]!.await, 'exit')
  assert.equal(payloadOf(del[1]!).projectId, create.projectId)
})

test('Given the branch rows, When their cases are read, Then vcs.init is answered for the workspace before the branch is refreshed or subscribed to', () => {
  for (const [id, row] of [['t3-method-vcs.refreshBranch', 'vcs.refreshBranch'], ['t3-method-subscribeVcsBranch', 'subscribeVcsBranch']] as const) {
    const s = sends(scriptOf(cases().get(id)!))
    assert.deepEqual(s.map(typeOf), ['vcs.init', row], id)
    assert.equal(s[0]!.await, 'exit', id)
    assert.deepEqual([payloadOf(s[0]!).cwd, payloadOf(s[1]!).cwd], ['{workspace}', '{workspace}'], id)
  }
})

test('Given a crawl without the vcs.init frame, When the agent-method scenarios are derived, Then it fails naming the method', () => {
  const t3 = loadApi('t3')
  const without = { ...t3, payloads: new Map([...t3.payloads].filter(([row]) => row !== 'rpc:vcs.init')) }
  assert.throws(() => t3AgentMethodScenarios(without), /vcs\.init/)
})
