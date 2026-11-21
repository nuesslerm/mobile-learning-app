// Given the committed crawl of T3, When the agent-query scenarios are derived, Then the committed set is what the generator makes, each case is labelled by
// the query row it asks, and the query follows an answered thread.create so its window holds the method answered on a thread the server knows.
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { test } from 'node:test'
import { repoRoot } from '../src/lock.ts'
import { loadApi } from '../src/scenarios/api.ts'
import { AGENT_QUERIES_DIR, agentMethods, agentQueries, agentTurns, render } from '../src/scenarios/index.ts'
import { t3AgentQueryScenarios } from '../src/scenarios/t3-agent.ts'

type Step = Record<string, unknown>
const scriptOf = (c: { input: unknown }) => (c.input as { script: Step[] }).script
const payloadOf = (s: Step) => (s['ws-send'] as { payload: Record<string, unknown> }).payload
const sends = (script: Step[]) => script.filter((s) => s['ws-send'])
const tagOf = (s: Step) => (s['ws-send'] as { tag: string }).tag
const cases = () => new Map(agentQueries().cases.map((c) => [c.id, c]))
const read = (file: string) => readFileSync(join(repoRoot, AGENT_QUERIES_DIR, file), 'utf8')

test('Given the committed crawl, When the agent-query scenarios are derived, Then the committed set is exactly that', () => {
  const out = render('t3', agentQueries(), 't3-agent-queries')
  assert.equal(read('corpus.json'), out.corpus, 'corpus.json is stale: node src/cli.ts scenarios t3-queries')
  assert.equal(read('scenarios.json'), out.scenarios)
})

test('Given the three agent sets, When their case ids are read, Then no id is in two of them', () => {
  const others = new Set([...agentTurns().cases, ...agentMethods().cases].map((c) => c.id))
  for (const c of agentQueries().cases) assert.ok(!others.has(c.id), c.id)
})

test('Given the cases, When one is labelled, Then its row is the query row it asks', () => {
  assert.deepEqual(Object.fromEntries([...cases()].map(([id, c]) => [id, c.row])), {
    't3-query-getThreadTitles': 'rpc:orchestration.getThreadTitles',
    't3-query-getThreadLinkMetadata': 'rpc:orchestration.getThreadLinkMetadata',
    't3-query-getNativeChildWork': 'rpc:orchestration.getNativeChildWork',
    't3-query-subscribePromptSuggestion': 'rpc:orchestration.subscribePromptSuggestion'
  })
})

test('Given each case, When it is read, Then the thread is made and answered before the query, which names that thread', () => {
  for (const [id, c] of cases()) {
    const s = sends(scriptOf(c))
    assert.deepEqual(s.map(tagOf), ['thread.create', c.row.replace('rpc:', '')].map((t) => (t === 'thread.create' ? tagOf(s[0]!) : t)), id)
    assert.equal(s[0]!.await, 'exit', id)
    const created = payloadOf(s[0]!).threadId
    const q = payloadOf(s[1]!)
    assert.deepEqual(q.threadIds ?? q.threadId, q.threadIds ? [created] : created, id)
  }
})

test('Given a crawl without the frame of a query, When the agent-query scenarios are derived, Then it fails naming the method', () => {
  const t3 = loadApi('t3')
  const without = { ...t3, payloads: new Map([...t3.payloads].filter(([row]) => row !== 'rpc:orchestration.getThreadTitles')) }
  assert.throws(() => t3AgentQueryScenarios(without), /getThreadTitles/)
})
