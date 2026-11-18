// Given the committed crawl, When the real-agent scenarios are derived, Then the committed set is what the generator makes, every case runs a real opencode turn
// ('reply with OK') before its row and scripts nothing, and the set's rules ignore the reply text and the ids while its mimic.toml names the key (never a value).
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { test } from 'node:test'
import { repoRoot } from '../src/lock.ts'
import { AGENT_REAL_DIR, agentCues, agentMethods, agentQueries, agentReal, agentTurns, render } from '../src/scenarios/index.ts'

type Step = Record<string, unknown>
const scriptOf = (c: { input: unknown }) => (c.input as { script: Step[] }).script
const payloadOf = (s: Step) => (s['ws-send'] as { payload: Record<string, unknown> }).payload
const sends = (script: Step[]) => script.filter((s) => s['ws-send'])
const types = (script: Step[]) => sends(script).map((s) => payloadOf(s).type ?? (s['ws-send'] as { tag: string }).tag)
const cases = () => new Map(agentReal().cases.map((c) => [c.id, c]))
const read = (file: string) => readFileSync(join(repoRoot, AGENT_REAL_DIR, file), 'utf8')

test('Given the committed crawl, When the real-agent scenarios are derived, Then the committed set is exactly that', () => {
  const out = render('t3', agentReal(), 't3-agent-real')
  assert.equal(read('corpus.json'), out.corpus, 'corpus.json is stale: node src/cli.ts scenarios t3-real')
  assert.equal(read('scenarios.json'), out.scenarios)
})

test('Given the five agent sets, When their case ids are read, Then no id is in two of them', () => {
  const others = new Set([...agentTurns().cases, ...agentMethods().cases, ...agentQueries().cases, ...agentCues().cases].map((c) => c.id))
  for (const c of agentReal().cases) assert.ok(!others.has(c.id), c.id)
})

test('Given the cases, When one is labelled, Then its row is the row it runs', () => {
  assert.deepEqual(Object.fromEntries([...cases()].map(([id, c]) => [id, c.row])), {
    't3-real-checkpoint.rollback': 'command:checkpoint.rollback',
    't3-real-thread.merge_back': 'command:thread.merge_back',
    't3-real-queued-message.promote-to-steer': 'command:queued-message.promote-to-steer'
  })
})

test('Given every case, When it is read, Then a real opencode turn with the trivial prompt starts first and nothing is scripted', () => {
  for (const c of cases().values()) {
    const script = scriptOf(c)
    assert.ok(!script.some((s) => s['cli.script']), `${c.id} scripts a CLI`)
    assert.ok(c.tags.includes('needs:real-agent') && !c.tags.includes('needs:cli-script'), c.id)
    const [settings, create, dispatch] = sends(script)
    assert.equal((settings!['ws-send'] as { tag: string }).tag, 'server.updateSettings', `${c.id} must enable the opencode provider first`)
    assert.deepEqual(payloadOf(settings!).providerInstanceMutation, { operation: 'create', instanceId: 'opencode', instance: { driver: 'opencode', enabled: true } }, c.id)
    assert.equal((payloadOf(create!).modelSelection as { instanceId: string }).instanceId, 'opencode', c.id)
    assert.equal(payloadOf(dispatch!).text, c.id.includes('queued') ? 'write the numbers from 1 to 400, one per line, with no other text' : 'reply with OK', c.id)
    if (!c.id.includes('queued')) assert.ok(script.some((s) => (s.listen as { ms?: number } | undefined)?.ms), `${c.id} must wait for the turn to finish`)
    assert.equal(create!.await, 'exit')
    assert.equal(dispatch!.await, 'exit')
  }
})

test('Given the rollback case, When it is read, Then the rollback follows a projection read that keeps the checkpoint of the finished turn', () => {
  const script = scriptOf(cases().get('t3-real-checkpoint.rollback')!)
  assert.deepEqual(types(script), ['server.updateSettings', 'thread.create', 'message.dispatch', 'orchestration.getThreadProjection', 'checkpoint.rollback', 'orchestration.getThreadProjection'])
  const read = script.find((s) => s.capture)!
  assert.ok(Object.keys(read.capture as object).includes('checkpointId'))
  assert.equal(payloadOf(sends(script)[4]!).checkpointId, '{checkpointId}')
})

test('Given the merge case, When it is read, Then the fork is answered before the merge back on a thread that has a finished turn', () => {
  const script = scriptOf(cases().get('t3-real-thread.merge_back')!)
  assert.deepEqual(types(script), ['server.updateSettings', 'thread.create', 'message.dispatch', 'orchestration.getThreadProjection', 'thread.fork', 'thread.merge_back', 'orchestration.getThreadProjection'])
  assert.equal(sends(script)[4]!.await, 'exit')
})

test('Given the steer case, When it is read, Then a second message is queued behind the first and the promote names both runs from the projection', () => {
  const script = scriptOf(cases().get('t3-real-queued-message.promote-to-steer')!)
  assert.deepEqual(types(script), ['server.updateSettings', 'thread.create', 'message.dispatch', 'message.dispatch', 'orchestration.getThreadProjection', 'queued-message.promote-to-steer', 'orchestration.getThreadProjection'])
  const promote = payloadOf(sends(script)[5]!)
  assert.deepEqual([promote.queuedRunId, promote.targetRunId], ['{q1}', '{runId}'])
})

test('Given the set rules, When they are read, Then the reply text and every id are masked and the usual masks are kept', () => {
  const rules = (JSON.parse(read('rules.json')) as { rules: { kind: string; keys?: Record<string, string>; pattern?: string }[] }).rules
  const masked = rules.filter((r) => r.kind === 'volatile-string').flatMap((r) => Object.keys(r.keys ?? {}))
  for (const key of ['text', 'messageId', 'runId', 'turnId', 'checkpointId', 'scopeId']) assert.ok(masked.includes(key), key)
  assert.ok(rules.some((r) => r.kind === 'token' && r.pattern?.includes('msg|ses')))
  const cues = (JSON.parse(readFileSync(join(repoRoot, 'clis/t3-serve/cassettes/t3-v0.1.9-agent-cues/rules.json'), 'utf8')) as { rules: unknown[] }).rules
  assert.deepEqual(rules.slice(0, cues.length), cues)
})

test('Given the set config, When mimic.toml is read, Then the key is declared by names alone and the provision runs the pinned real opencode', () => {
  const toml = read('mimic.toml')
  assert.match(toml, /^env_from_vault = \{ MIMIC_CRED_OPENCODE__OPENCODE_API_KEY = "OPENCODE_GO_API_KEY" \}$/m)
  assert.match(toml, /provision_file = "\/\/tooling\/mimic-drivers\/provision\/t3-real\.toml"/)
  assert.doesNotMatch(toml, /sk-|Bearer /)
  const provision = readFileSync(join(repoRoot, 'tooling/mimic-drivers/provision/t3-real.toml'), 'utf8')
  assert.match(provision, /^real_agents = \["opencode"\]$/m)
  assert.doesNotMatch(provision, /scripted-clis\/opencode/)
})
