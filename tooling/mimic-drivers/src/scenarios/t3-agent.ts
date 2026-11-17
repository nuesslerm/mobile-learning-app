// T3 agent-turn scenarios: the rows whose recorded call must reach an agent (a turn that is still running, a queue behind it, a thread of an agent
// instance the chain can drive). The first set (`t3.ts`) asks them on the prelude's codex thread, which rca has no driver for. Here every case makes its
// own thread on Claude's instance and scripts the claude CLI through `cli.script` only for the cue the rule names: a turn that never ends, so the run is
// still live when it is interrupted, stopped or queued behind. A row that needs a turn to finish or an approval to be raised is not here: the scripted
// CLI cannot speak the protocol the real T3 holds with its CLI, and a real agent needs a credential the reference host must not hold (R3 State line).
// A second set, `t3-v0.1.9-agent-methods`, holds the cases whose row has to be the label of the case that runs its agent method: a recorded set's corpus
// is frozen with its cassette, so these are not edits of the first. checkpoint.rollback is not in it: it needs a finished turn of a real agent.
import type { Capture } from '../capture.ts'
import type { Case } from '../corpus/types.ts'
import type { Json } from '../protocol.ts'
import { fail } from './common.ts'
import type { Api, Covered, ScenarioSet } from './types.ts'

const AGENT_THREAD = 'agent-thread'
const FORK_THREAD = 'agent-fork'
const OWN_PROJECT = 'agent-project'
const LATEST_STABLE = { type: 'latest_stable' }
const REPO = { cwd: '{workspace}' }
const OWN_ROOT = { projectId: OWN_PROJECT, title: 'Agent project', workspaceRoot: `{workspace}/${OWN_PROJECT}`, createWorkspaceRootIfMissing: true }
const PROJECT = 'conformance-project'
const FIRST = 'm-first'
const PROJECTION = 'orchestration.getThreadProjection'
const RUN: Capture = { runId: { from: ['/exit/value/runs/0/runId', '/exit/value/runs/0/id'] } }
const QUEUED: Capture = {
  ...RUN,
  q1: { from: ['/exit/value/runs/1/runId', '/exit/value/runs/1/id'] },
  q2: { from: ['/exit/value/runs/2/runId', '/exit/value/runs/2/id'] }
}

type Entry = Record<string, Json>
type Step = { m: string; set?: Record<string, Json>; capture?: Capture; wait?: boolean }
type Agent = { id: string; rows: string[]; note: string; hung: boolean; queue: number; steps: Step[]; /** `false`: the case makes no thread of its own. */ thread?: false }

const THIS = { threadId: AGENT_THREAD }
const stop = { ...THIS, reason: 'user' }

const TURNS: Agent[] = [
  { id: 'run.interrupt', rows: ['run.interrupt'], note: 'interrupt the run of a turn that is still running', hung: true, queue: 0, steps: [{ m: 'run.interrupt', set: { ...THIS, runId: '{runId}' } }, { m: PROJECTION, set: THIS }] },
  { id: 'thread.stop', rows: ['thread.stop'], note: 'stop a thread whose turn is still running', hung: true, queue: 0, steps: [{ m: 'thread.stop', set: stop }, { m: PROJECTION, set: THIS }] },
  { id: 'queued-run.edit', rows: ['queued-run.edit'], note: 'edit a message queued behind a running turn', hung: true, queue: 1, steps: [{ m: 'queued-run.edit', set: { ...THIS, runId: '{q1}', text: 'edited' } }, { m: PROJECTION, set: THIS }] },
  { id: 'queued-run.cancel', rows: ['queued-run.cancel'], note: 'cancel a message queued behind a running turn', hung: true, queue: 1, steps: [{ m: 'queued-run.cancel', set: { ...THIS, runId: '{q1}' } }, { m: PROJECTION, set: THIS }] },
  { id: 'queued-run.reorder', rows: ['queued-run.reorder'], note: 'move the second queued message before the first', hung: true, queue: 2, steps: [{ m: 'queued-run.reorder', set: { ...THIS, runId: '{q2}', beforeRunId: '{q1}' } }, { m: PROJECTION, set: THIS }] },
  { id: 'queued-run.start', rows: ['queued-run.start', 'run.interrupt'], note: 'interrupt the running turn and hold the queue, then start the first queued message by hand', hung: true, queue: 1, steps: [{ m: 'run.interrupt', set: { ...THIS, runId: '{runId}', holdQueue: true } }, { m: 'queued-run.start', set: { ...THIS, runId: '{q1}' } }, { m: PROJECTION, set: THIS }] },
  { id: 'queued-message.promote-to-steer', rows: ['queued-message.promote-to-steer'], note: 'promote a queued message to steer the running turn', hung: true, queue: 1, steps: [{ m: 'queued-message.promote-to-steer', set: { ...THIS, queuedRunId: '{q1}', targetRunId: '{runId}' } }, { m: PROJECTION, set: THIS }] },
  { id: 'thread.snooze', rows: ['thread.snooze'], note: 'snooze a thread of Claude that has no turn', hung: false, queue: 0, steps: [{ m: 'thread.snooze', set: { ...THIS, snoozedUntil: '2031-01-01T00:00:00.000Z' } }, { m: PROJECTION, set: THIS }] },
  { id: 'thread.fork', rows: ['thread.fork'], note: 'fork a thread of Claude that has no turn', hung: false, queue: 0, steps: [{ m: 'thread.fork', set: { sourceThreadId: AGENT_THREAD, targetThreadId: FORK_THREAD, sourcePoint: {} } }, { m: PROJECTION, set: { threadId: FORK_THREAD } }] },
  { id: 'thread.merge_back', rows: ['thread.fork', 'thread.merge_back'], note: 'fork a thread of Claude that has no turn and merge the fork back', hung: false, queue: 0, steps: [{ m: 'thread.fork', set: { sourceThreadId: AGENT_THREAD, targetThreadId: FORK_THREAD, sourcePoint: {} } }, { m: 'thread.merge_back', set: { sourceThreadId: FORK_THREAD, targetThreadId: AGENT_THREAD, sourcePoint: {} } }, { m: PROJECTION, set: THIS }] }
]

/** The label of a case is its last row: each of these ends with the row whose agent method the recording must show. */
const METHODS: Agent[] = [
  { id: 'queued-run.start', rows: ['run.interrupt', 'queued-run.start'], note: 'interrupt the running turn and hold the queue, then start the first queued message by hand', hung: true, queue: 1, steps: [{ m: 'run.interrupt', set: { ...THIS, runId: '{runId}', holdQueue: true } }, { m: 'queued-run.start', set: { ...THIS, runId: '{q1}' } }, { m: PROJECTION, set: THIS }] },
  { id: 'thread.fork', rows: ['thread.fork'], note: 'fork a thread of Claude that has no turn from its latest stable point', hung: false, queue: 0, steps: [{ m: 'thread.fork', set: { sourceThreadId: AGENT_THREAD, targetThreadId: FORK_THREAD, sourcePoint: LATEST_STABLE } }, { m: PROJECTION, set: { threadId: FORK_THREAD } }] },
  { id: 'thread.merge_back', rows: ['thread.fork', 'thread.merge_back'], note: 'fork a thread of Claude that has no turn and merge the fork back', hung: false, queue: 0, steps: [{ m: 'thread.fork', set: { sourceThreadId: AGENT_THREAD, targetThreadId: FORK_THREAD, sourcePoint: LATEST_STABLE }, wait: true }, { m: 'thread.merge_back', set: { sourceThreadId: FORK_THREAD, targetThreadId: AGENT_THREAD, sourcePoint: LATEST_STABLE } }, { m: PROJECTION, set: THIS }] },
  { id: 'thread.create', rows: ['thread.create'], note: 'create a thread of Claude in the prelude project', hung: false, queue: 0, steps: [{ m: PROJECTION, set: THIS }] },
  { id: 'project.create', rows: ['project.create'], note: 'create a project in a folder below the workspace that does not exist yet', hung: false, queue: 0, thread: false, steps: [{ m: 'project.create', set: OWN_ROOT }] },
  { id: 'project.delete', rows: ['project.create', 'project.delete'], note: 'create a project below the workspace and delete it', hung: false, queue: 0, thread: false, steps: [{ m: 'project.create', set: OWN_ROOT, wait: true }, { m: 'project.delete', set: { projectId: OWN_PROJECT } }] },
  { id: 'vcs.refreshBranch', rows: ['vcs.init', 'vcs.refreshBranch'], note: 'make the workspace a repository, then refresh its branch', hung: false, queue: 0, thread: false, steps: [{ m: 'vcs.init', set: REPO, wait: true }, { m: 'vcs.refreshBranch', set: REPO }] },
  { id: 'subscribeVcsBranch', rows: ['vcs.init', 'subscribeVcsBranch'], note: 'make the workspace a repository, then subscribe to its branch', hung: false, queue: 0, thread: false, steps: [{ m: 'vcs.init', set: REPO, wait: true }, { m: 'subscribeVcsBranch', set: REPO }] }
]

function frameOf(api: Api, name: string): Entry {
  const matches = api.endpoints.filter((e) => e.name === name)
  if (matches.length !== 1) return fail(`${name} is ${matches.length} endpoints of the crawl, expected one`)
  const frame = api.payloads.get(matches[0]!.id)?.frame
  if (frame === undefined) return fail(`${name} has no valid request frame in the crawl`)
  return frame as Entry
}

/** The crawl's frame of `m` with its own request and command id (the server answers a repeated command id with the first answer) and `set` on top. */
function send(api: Api, m: string, id: string, set: Record<string, Json> = {}, extra: Entry = {}): Entry {
  const frame = frameOf(api, m)
  const payload = { ...((frame.payload ?? {}) as Record<string, Json>) }
  if ('commandId' in payload) payload.commandId = `cmd-${id}`
  return { 'ws-send': { ...frame, id, payload: { ...payload, ...set } }, ...extra }
}

const CLAUDE = { instanceId: 'claudeAgent', model: 'sonnet' }

function steps(api: Api, agent: Agent): Entry[] {
  const out: Entry[] = []
  if (agent.hung) out.push({ 'cli.script': { cli: 'claude', turns: [{ hang: true }] } as unknown as Json })
  if (agent.thread !== false) out.push(send(api, 'thread.create', 'c1', { threadId: AGENT_THREAD, projectId: PROJECT, title: 'Agent thread', modelSelection: CLAUDE as unknown as Json, branch: null, worktreePath: null }, { await: 'exit' }))
  if (agent.hung) {
    out.push(send(api, 'message.dispatch', 't1', { threadId: AGENT_THREAD, messageId: FIRST, text: 'hello', dispatchMode: { type: 'start_immediately' } }, { await: 'exit' }))
    for (let i = 1; i <= agent.queue; i++) out.push(send(api, 'message.dispatch', `t${i + 1}`, { threadId: AGENT_THREAD, messageId: `m-queued-${i}`, text: `queued ${i}`, dispatchMode: { type: 'queue_after_active' } }, { await: 'exit' }))
    out.push({ ...send(api, PROJECTION, 'r1', THIS), capture: (agent.queue ? QUEUED : RUN) as unknown as Json })
  }
  agent.steps.forEach((s, i) => out.push(send(api, s.m, `s${i + 1}`, s.set, s.wait ? { await: 'exit' } : {})))
  return out
}

function derive(api: Api, agents: Agent[], prefix: string): ScenarioSet {
  const cases: Case[] = []
  const scenarios: ScenarioSet['scenarios'] = []
  const via = new Map<string, string[]>()
  for (const a of agents) {
    const id = `${prefix}-${a.id}`
    const script = { prelude: 'session', script: [...steps(api, a), { 'ws-close': {} }], settle_ms: a.hung ? 1500 : 400 } as unknown as Json
    const rows = [...new Set(a.rows.map((m) => frameOfRow(api, m)))]
    cases.push({ id, row: frameOfRow(api, a.rows.at(-1)!), op: 'script', input: script, tags: ['input:scenario', 'state:fresh', 'kind:story', ...(a.hung ? ['needs:cli-script'] : [])] })
    scenarios.push({ id, kind: 'story', rows, note: a.note })
    for (const r of rows) via.set(r, [...(via.get(r) ?? []), id])
  }
  const coverage: Covered[] = [...via].map(([row, ids]) => ({ row, state: 'populated', via: ids }))
  return { cases, scenarios, coverage }
}

export const t3AgentScenarios = (api: Api): ScenarioSet => derive(api, TURNS, 't3-agent')
export const t3AgentMethodScenarios = (api: Api): ScenarioSet => derive(api, METHODS, 't3-method')

function frameOfRow(api: Api, name: string): string {
  const row = api.endpoints.find((e) => e.name === name)
  return row ? row.id : fail(`${name} is no endpoint of the crawl`)
}
