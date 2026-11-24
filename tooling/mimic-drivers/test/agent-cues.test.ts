// Given the committed crawl of T3, When the agent-cue scenarios are derived, Then the committed set is what the generator makes, each case is labelled by
// the row it runs, an unarchive follows an answered archive, and a subagent stop names the run of a turn that is still running.
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { test } from 'node:test'
import { repoRoot } from '../src/lock.ts'
import { AGENT_CUES_DIR, agentCues, agentMethods, agentQueries, agentTurns, render } from '../src/scenarios/index.ts'

type Step = Record<string, unknown>
const scriptOf = (c: { input: unknown }) => (c.input as { script: Step[] }).script
const payloadOf = (s: Step) => (s['ws-send'] as { payload: Record<string, unknown> }).payload
const sends = (script: Step[]) => script.filter((s) => s['ws-send'])
const types = (script: Step[]) => sends(script).map((s) => payloadOf(s).type ?? (s['ws-send'] as { tag: string }).tag)
const cases = () => new Map(agentCues().cases.map((c) => [c.id, c]))
const read = (file: string) => readFileSync(join(repoRoot, AGENT_CUES_DIR, file), 'utf8')

test('Given the committed crawl, When the agent-cue scenarios are derived, Then the committed set is exactly that', () => {
  const out = render('t3', agentCues(), 't3-agent-cues')
  assert.equal(read('corpus.json'), out.corpus, 'corpus.json is stale: node src/cli.ts scenarios t3-cues')
  assert.equal(read('scenarios.json'), out.scenarios)
})

test('Given the four agent sets, When their case ids are read, Then no id is in two of them', () => {
  const others = new Set([...agentTurns().cases, ...agentMethods().cases, ...agentQueries().cases].map((c) => c.id))
  for (const c of agentCues().cases) assert.ok(!others.has(c.id), c.id)
})

test('Given the cases, When one is labelled, Then its row is the row it runs', () => {
  assert.deepEqual(Object.fromEntries([...cases()].map(([id, c]) => [id, c.row])), {
    't3-cue-thread.unarchive': 'command:thread.unarchive',
    't3-cue-provider-session.detach': 'command:provider-session.detach',
    't3-cue-subagent.stop': 'command:subagent.stop'
  })
})

test('Given the unarchive case, When it is read, Then the thread is archived and the archive answered before the unarchive', () => {
  const s = sends(scriptOf(cases().get('t3-cue-thread.unarchive')!))
  assert.deepEqual(types(scriptOf(cases().get('t3-cue-thread.unarchive')!)), ['thread.create', 'thread.archive', 'thread.unarchive', 'orchestration.getThreadProjection'])
  assert.equal(s[0]!.await, 'exit')
  assert.equal(s[1]!.await, 'exit')
})

test('Given the subagent case, When it is read, Then a hung claude turn runs first and the stop names its run', () => {
  const c = cases().get('t3-cue-subagent.stop')!
  const script = scriptOf(c)
  assert.deepEqual((script[0]!['cli.script'] as { cli: string; turns: unknown[] }), { cli: 'claude', turns: [{ hang: true }] })
  const stop = sends(script).find((s) => payloadOf(s).type === 'subagent.stop')!
  assert.equal(payloadOf(stop).runId, '{runId}')
  assert.ok(c.tags.includes('needs:cli-script'))
})
