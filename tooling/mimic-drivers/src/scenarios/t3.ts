// T3 scenarios. After the session prelude (a project and a thread exist) a method that names `threadId`, `projectId` or `cwd` is asked about the
// entity the prelude made, not an unknown id; a method that names a message or a run is asked after a turn the scripted CLI answers
// (`T3_CLI_DIR`: exec-testkit's `claude`/`codex`, no model credentials), whose run id is read from the thread projection.
import type { Capture } from '../capture.ts'
import type { Case } from '../corpus/types.ts'
import { PROJECT, THREAD } from '../corpus/t3.ts'
import type { Json } from '../protocol.ts'
import { coverageOf, fail } from './common.ts'
import type { Api, Endpoint, ScenarioSet } from './types.ts'

/** Keys that name the prelude's entities, and the value that makes them name it. */
const PRELUDE_KEYS: Record<string, string> = { threadId: THREAD, projectId: PROJECT, parentThreadId: THREAD, targetThreadId: THREAD, cwd: '{workspace}' }
const TURN_MESSAGE = 'm-turn'
const TURN_KEYS: Record<string, string> = { messageId: TURN_MESSAGE, runId: '{runId}' }
const RUN_ID: Capture = { runId: { from: ['/exit/value/runs/0/runId', '/exit/value/runs/0/id'] } }
const PROJECTION = 'orchestration.getThreadProjection'
const UNRESOLVED = /^(requestId|itemId|taskId|attemptId|nodeId|checkpointId|deliveryId)$/

type Entry = Record<string, Json>
type Level = 'prelude' | 'turn'
type StoryStep = { m: string; set?: Record<string, Json>; use?: 'valid' | 'boundary'; capture?: Capture }
type Story = { id: string; turn: boolean; steps: StoryStep[]; note: string }

const STORIES: Story[] = [
  { id: 'thread-lifecycle', turn: false, note: 'rename, pin, archive, unarchive and delete a thread, reading it between', steps: [{ m: PROJECTION }, { m: 'thread.metadata.update', set: { title: 'renamed' } }, { m: PROJECTION }, { m: 'thread.pin' }, { m: 'thread.unpin' }, { m: 'thread.archive' }, { m: 'thread.unarchive' }, { m: 'thread.mark-unread' }, { m: 'thread.delete' }, { m: PROJECTION }] },
  { id: 'project-lifecycle', turn: false, note: 'update and delete a project', steps: [{ m: 'project.update' }, { m: 'project.delete' }] },
  { id: 'turn-and-history', turn: true, note: 'one scripted turn, its projection, an interrupt and the projection again', steps: [{ m: PROJECTION }, { m: 'run.interrupt' }, { m: PROJECTION }, { m: 'thread.stop' }, { m: PROJECTION }] }
]

type Row = { endpoint: Endpoint; frame: Entry }

function frameOf(api: Api, name: string): Row {
  const matches = api.endpoints.filter((e) => e.name === name)
  if (matches.length !== 1) return fail(`${name} is ${matches.length} endpoints of the crawl, expected one`)
  const frame = api.payloads.get(matches[0]!.id)?.frame
  if (frame === undefined) return fail(`${name} has no valid request frame in the crawl`)
  return { endpoint: matches[0]!, frame: frame as Entry }
}

const payloadOf = (frame: Entry): Record<string, Json> => ({ ...((frame.payload ?? {}) as Record<string, Json>) })

/** The frame with the entity keys it names replaced (only where it holds the schema's placeholder), a request and command id of its own (the server
 * answers a repeated command id with the first answer), and `set` on top. */
function request(row: Row, id: string, levels: Set<Level>, set: Record<string, Json> = {}, from: Json = row.frame): Entry {
  const payload = payloadOf({ payload: (from as Entry).payload ?? row.frame.payload })
  if (payload.commandId === 'a') payload.commandId = `cmd-${id}`
  for (const [k, v] of Object.entries(PRELUDE_KEYS)) if (payload[k] === 'a') payload[k] = v
  if (levels.has('turn')) for (const [k, v] of Object.entries(TURN_KEYS)) if (payload[k] === 'a') payload[k] = v
  return { 'ws-send': { ...row.frame, id, payload: { ...payload, ...set } } }
}

function turn(api: Api): Entry[] {
  const dispatch = frameOf(api, 'message.dispatch')
  const start = request(dispatch, 't1', new Set(['prelude']), { commandId: 'cmd-turn', messageId: TURN_MESSAGE, text: 'hello', dispatchMode: { type: 'start_immediately' } })
  return [start, { ...request(frameOf(api, PROJECTION), 't2', new Set(['prelude'])), capture: RUN_ID as unknown as Json }]
}

/** A turn's answers keep arriving while the scripted CLI runs: the requests after it wait longer for the wire to go quiet. */
const script = (entries: Entry[], turned: boolean): Json => ({ prelude: 'session', script: [...entries, { 'ws-close': {} }], settle_ms: turned ? 1500 : 400 }) as unknown as Json

const levelOf = (payload: Record<string, Json>): Level | null => {
  if (Object.keys(TURN_KEYS).some((k) => payload[k] === 'a')) return 'turn'
  return Object.keys(PRELUDE_KEYS).some((k) => payload[k] === 'a') ? 'prelude' : null
}

export function t3Scenarios(api: Api): ScenarioSet {
  const cases: Case[] = []
  const scenarios: ScenarioSet['scenarios'] = []
  const via = new Map<string, string[]>()
  const note = (row: string, id: string) => via.set(row, [...(via.get(row) ?? []), id])

  for (const e of api.endpoints) {
    const payloads = api.payloads.get(e.id)
    if (e.skipped || payloads?.frame === undefined) continue
    const level = levelOf(payloadOf({ payload: payloads.valid ?? {} }))
    if (level === null) continue
    const levels = new Set<Level>(level === 'turn' ? ['prelude', 'turn'] : ['prelude'])
    const row: Row = { endpoint: e, frame: payloads.frame as Entry }
    const subject = request(row, '1', levels)
    const subjectPayload = (subject['ws-send'] as Entry).payload as Record<string, Json>
    const readback = subjectPayload.threadId === THREAD && e.name !== PROJECTION ? [request(frameOf(api, PROJECTION), '2', new Set(['prelude']))] : []
    const id = `t3-scn-${e.kind}-${e.name}`
    cases.push({ id, row: e.id, op: 'script', input: script([...(level === 'turn' ? turn(api) : []), subject, ...readback], level === 'turn'), tags: ['input:scenario', 'state:fresh', 'kind:derived', `level:${level}`, ...(level === 'turn' ? ['needs:cli'] : [])] })
    scenarios.push({ id, kind: 'derived', rows: [e.id] })
    note(e.id, id)
  }

  for (const story of STORIES) {
    const levels = new Set<Level>(story.turn ? ['prelude', 'turn'] : ['prelude'])
    const entries = story.steps.map((s, i) => {
      const row = frameOf(api, s.m)
      const use = s.use === 'boundary' ? api.payloads.get(row.endpoint.id)?.boundary : undefined
      const entry = request(row, `s${i + 1}`, levels, s.set, use === undefined ? row.frame : ({ payload: use } as Json))
      const known = new Set([...Object.keys(payloadOf({ payload: api.payloads.get(row.endpoint.id)?.valid ?? {} })), ...Object.keys(payloadOf({ payload: api.payloads.get(row.endpoint.id)?.boundary ?? {} }))])
      for (const k of Object.keys(s.set ?? {})) if (!known.has(k)) fail(`story ${story.id}: ${s.m} has no key ${k} in the crawl's payloads`)
      return { row, entry: s.capture ? { ...entry, capture: s.capture as unknown as Json } : entry }
    })
    const id = `t3-story-${story.id}`
    const rows = [...new Set([...(story.turn ? ['message.dispatch'] : []), ...story.steps.map((s) => s.m)].map((m) => frameOf(api, m).endpoint.id))]
    cases.push({ id, row: entries[0]!.row.endpoint.id, op: 'script', input: script([...(story.turn ? turn(api) : []), ...entries.map((x) => x.entry)], story.turn), tags: ['input:scenario', 'state:fresh', 'kind:story', ...(story.turn ? ['needs:cli'] : [])] })
    scenarios.push({ id, kind: 'story', rows, note: story.note })
    for (const r of rows) note(r, id)
  }

  const coverage = coverageOf(api, via, (e) => {
    const payloads = api.payloads.get(e.id)
    if (e.skipped) return `not-recorded: ${e.skipped}`
    if (e.transport === 'http') return 'stateless: an http route names no entity'
    if (payloads?.valid === undefined) return 'unreadable-schema: only the empty payload is asked'
    const other = Object.keys(payloadOf({ payload: payloads.valid })).filter((k) => UNRESOLVED.test(k))
    if (other.length) return `unresolved-entity: names ${other.join(', ')}, raised by a provider run (an approval, a tool item, a checkpoint) the scripted CLI does not raise here`
    return 'stateless: names no stored entity, the empty-state answer is the answer'
  })
  return { cases, scenarios, coverage }
}
