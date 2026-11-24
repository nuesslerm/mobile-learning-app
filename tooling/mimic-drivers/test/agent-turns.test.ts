// Given the committed crawl of T3, When the agent-turn scenarios are derived, Then the committed set is what the generator makes, every case makes its
// own thread on Claude's instance, a case that needs a live run scripts the claude CLI to hang before anything is sent, and a queued run, an interrupt
// and a steer name the run ids an earlier projection answers with.
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { test } from 'node:test'
import { repoRoot } from '../src/lock.ts'
import { loadApi } from '../src/scenarios/api.ts'
import { AGENT_TURNS_DIR, agentTurns, render } from '../src/scenarios/index.ts'
import { t3AgentScenarios } from '../src/scenarios/t3-agent.ts'

type Step = Record<string, unknown>
const scriptOf = (c: { input: unknown }) => (c.input as { script: Step[] }).script
const payloadOf = (s: Step) => ((s['ws-send'] as { payload: Record<string, unknown> }).payload)
const read = (file: string) => readFileSync(join(repoRoot, AGENT_TURNS_DIR, file), 'utf8')

test('Given the committed crawl, When the agent-turn scenarios are derived, Then the committed set is exactly that', () => {
  const out = render('t3', agentTurns(), 't3-agent-turns')
  assert.equal(read('corpus.json'), out.corpus, 'corpus.json is stale: node src/cli.ts scenarios t3-agent')
  assert.equal(read('scenarios.json'), out.scenarios)
})

test('Given the scenarios, When a case is read, Then it creates its own Claude thread, and a hung one scripts the CLI before the first request', () => {
  const set = agentTurns()
  assert.ok(set.cases.length >= 8)
  for (const c of set.cases) {
    const script = scriptOf(c)
    const create = script.find((s) => s['ws-send'] && payloadOf(s).type === 'thread.create')
    assert.deepEqual(payloadOf(create!).modelSelection, { instanceId: 'claudeAgent', model: 'sonnet' }, c.id)
    const hung = c.tags.includes('needs:cli-script')
    assert.equal(script[0]!['cli.script'] !== undefined, hung, c.id)
    if (hung) assert.deepEqual(script[0]!['cli.script'], { cli: 'claude', turns: [{ hang: true }] })
    assert.ok(!JSON.stringify(script).includes('"mcp_call"'), `${c.id} scripts only a turn the reference host can run`)
  }
})

test('Given a queue behind a running turn, When a case acts on a queued run, Then the run ids come from the projection read after the messages were queued', () => {
  const cases = new Map(agentTurns().cases.map((c) => [c.id, scriptOf(c)]))
  const reorder = cases.get('t3-agent-queued-run.reorder')!
  const queued = reorder.filter((s) => s['ws-send'] && (payloadOf(s).dispatchMode as { type: string } | undefined)?.type === 'queue_after_active')
  assert.equal(queued.length, 2)
  const read = reorder.findIndex((s) => s.capture !== undefined)
  assert.ok(read > reorder.indexOf(queued[1]!), 'the ids are read after both messages are queued')
  const move = payloadOf(reorder.find((s) => s['ws-send'] && payloadOf(s).type === 'queued-run.reorder')!)
  assert.deepEqual([move.runId, move.beforeRunId], ['{q2}', '{q1}'])
  const start = cases.get('t3-agent-queued-run.start')!.filter((s) => s['ws-send']).map((s) => payloadOf(s).type)
  assert.ok(start.indexOf('run.interrupt') < start.indexOf('queued-run.start'), 'the queue is held by an interrupt before a queued run is started by hand')
  assert.equal(payloadOf(cases.get('t3-agent-run.interrupt')!.find((s) => s['ws-send'] && payloadOf(s).type === 'run.interrupt')!).runId, '{runId}')
})

test('Given a crawl without the message.dispatch frame, When the agent-turn scenarios are derived, Then it fails naming the method', () => {
  const t3 = loadApi('t3')
  const without = { ...t3, payloads: new Map([...t3.payloads].filter(([row]) => row !== 'command:message.dispatch')) }
  assert.throws(() => t3AgentScenarios(without), /message\.dispatch/)
})
