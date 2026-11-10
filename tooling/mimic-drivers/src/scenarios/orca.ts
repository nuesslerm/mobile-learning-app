// Orca scenarios. The relations are the entity keys of the pinned schemas: `repo`, `worktree` and `terminal` are selectors of something an earlier
// call created (92, 92 and 23 of the 832 methods name them), each made by one provider method on top of the one before it. A method whose valid
// payload names one is asked after the chain that makes it; `{git.main}` is a real repository the driver makes (one commit, fixed dates).
import type { Capture } from '../capture.ts'
import type { Case } from '../corpus/types.ts'
import type { Json } from '../protocol.ts'
import { coverageOf, fail } from './common.ts'
import type { Api, ScenarioSet } from './types.ts'

type Level = 'repo' | 'worktree' | 'terminal'
const LEVELS: Level[] = ['repo', 'worktree', 'terminal']

/** The method that makes an entity, what its answer calls the new one, and the level it stands on. */
const PROVIDERS: Record<Level, { method: string; set?: Record<string, Json>; capture: Capture; parent?: Level }> = {
  repo: { method: 'repo.add', set: { path: '{git.main}' }, capture: { repo: { from: ['/result/repo/id'], prefix: 'id:' } } },
  worktree: { method: 'worktree.create', parent: 'repo', capture: { worktree: { from: ['/result/worktree/id', '/result/id'], prefix: 'id:' } } },
  terminal: { method: 'terminal.create', parent: 'worktree', capture: { terminal: { from: ['/result/terminal/handle', '/result/handle'] } } }
}

/** A path key of the schemas names a file the repository has once a worktree of it exists. */
const FILE_KEYS: Record<string, string> = { relativePath: 'README.md' }
/** The list method of an entity namespace, asked after a method of it ran so the effect is in the cassette. */
const READBACK: Record<string, string> = { repo: 'repo.list', worktree: 'worktree.list', terminal: 'terminal.list', git: 'git.status' }
/** Keys of other entities no provider chain here makes: listed, not guessed. */
const OTHER_ENTITY = /^(accountId|sessionId|projectId|workspaceId|tabId|paneKey|profileId|teamId|flowId|dispatchId|subscriptionId|operationId|uploadId|connectionId|targetId|siteId)$/

const ROW = (m: string) => `rpc:${m}`
const TIMEOUT = 8000
/** Creating a worktree or a terminal runs git or a shell. */
const PROVIDER_TIMEOUT = 30_000

type Step = { method: string; params: Json; timeout_ms: number; capture?: Capture }
type StoryStep = { m: string; set?: Record<string, Json>; use?: 'valid' | 'boundary' }
type Story = { id: string; from: Level | null; steps: StoryStep[]; note: string }

/** Stories pick methods and their order only: the payloads are the crawl's, plus the few values that make the sequence meaningful. */
const STORIES: Story[] = [
  { id: 'repo-lifecycle', from: 'repo', note: 'add, inspect, change a base ref and remove a repository', steps: [{ m: 'repo.list' }, { m: 'repo.show' }, { m: 'repo.setBaseRef', set: { ref: 'main' } }, { m: 'repo.baseRefDefault' }, { m: 'repo.searchRefs', set: { query: 'ma' } }, { m: 'repo.sparsePresets' }, { m: 'repo.rm' }, { m: 'repo.list' }] },
  { id: 'worktree-lifecycle', from: 'worktree', note: 'create, list, inspect, sleep and remove a worktree', steps: [{ m: 'worktree.list' }, { m: 'worktree.show' }, { m: 'worktree.ps' }, { m: 'worktree.sleep' }, { m: 'worktree.rm' }, { m: 'worktree.list' }] },
  { id: 'files-roundtrip', from: 'worktree', note: 'create, write, read, rename and delete a file of a worktree', steps: [{ m: 'files.createFile', set: { relativePath: 'notes.txt' } }, { m: 'files.write', set: { relativePath: 'notes.txt', content: 'hello' } }, { m: 'files.read', set: { relativePath: 'notes.txt' } }, { m: 'files.stat', set: { relativePath: 'notes.txt' } }, { m: 'files.list' }, { m: 'files.delete', set: { relativePath: 'notes.txt' } }, { m: 'files.list' }] },
  { id: 'git-flow', from: 'worktree', note: 'write a file, see it as a change, stage, commit and read the history', steps: [{ m: 'files.createFile', set: { relativePath: 'change.txt' } }, { m: 'git.status' }, { m: 'git.stage', set: { filePath: 'change.txt' } }, { m: 'git.status' }, { m: 'git.commit', set: { message: 'add change' } }, { m: 'git.history' }, { m: 'git.status' }] },
  { id: 'terminal-lifecycle', from: 'terminal', note: 'create a terminal, list, show, rename and close it', steps: [{ m: 'terminal.list' }, { m: 'terminal.show' }, { m: 'terminal.rename', set: { title: 'renamed' } }, { m: 'terminal.close' }, { m: 'terminal.list' }] },
  { id: 'settings-roundtrip', from: null, note: 'read, change and read the settings', steps: [{ m: 'settings.get' }, { m: 'settings.update', use: 'boundary' }, { m: 'settings.get' }] },
  { id: 'state-after-repo', from: 'worktree', note: 'the host-wide state methods once a repository and a worktree exist', steps: [{ m: 'status.get' }, { m: 'stats.summary' }, { m: 'project.list' }, { m: 'repo.list' }, { m: 'worktree.list' }] }
]

const params = (payload: Json | undefined): Record<string, Json> => (payload !== null && typeof payload === 'object' && !Array.isArray(payload) ? { ...(payload as Record<string, Json>) } : {})

/** The entity keys a payload names with the schema's placeholder, replaced by the variables of the levels reached so far. */
function substitute(base: Record<string, Json>, reached: Set<Level>): Record<string, Json> {
  const out = { ...base }
  for (const level of LEVELS) if (reached.has(level) && out[level] !== undefined) out[level] = `{${level}}`
  if (reached.has('worktree')) for (const [k, v] of Object.entries(FILE_KEYS)) if (out[k] === 'a') out[k] = v
  return out
}

function chain(level: Level | null, api: Api): { steps: Step[]; reached: Set<Level> } {
  const levels: Level[] = []
  for (let l: Level | undefined = level ?? undefined; l; l = PROVIDERS[l].parent) levels.unshift(l)
  const reached = new Set<Level>()
  const steps = levels.map((l) => {
    const p = PROVIDERS[l]
    const base = params(api.payloads.get(ROW(p.method))?.valid)
    if (!api.payloads.has(ROW(p.method))) fail(`provider ${p.method} has no valid payload in the crawl`)
    const step: Step = { method: p.method, params: { ...substitute(base, reached), ...p.set }, timeout_ms: PROVIDER_TIMEOUT, capture: p.capture }
    reached.add(l)
    return step
  })
  return { steps, reached }
}

function step(api: Api, reached: Set<Level>, method: string, set: Record<string, Json> = {}, use: 'valid' | 'boundary' = 'valid'): Step {
  const payloads = api.payloads.get(ROW(method))
  const base = payloads?.[use] ?? payloads?.valid ?? payloads?.empty
  if (base === undefined) return fail(`${method} has no ${use} payload in the crawl`)
  const known = new Set([...Object.keys(params(payloads?.valid)), ...Object.keys(params(payloads?.boundary))])
  for (const k of Object.keys(set)) if (!known.has(k)) fail(`${method}: the crawl's payloads have no key ${k} to set`)
  return { method, params: { ...substitute(params(base), reached), ...set }, timeout_ms: TIMEOUT }
}

const input = (setup: boolean, requests: Step[]): Json => ({ ...(setup ? { setup: [{ git: 'main' }] } : {}), requests }) as unknown as Json

/** The deepest level a payload's keys name, or null when it names none. */
function levelOf(base: Record<string, Json>): Level | null {
  const named = LEVELS.filter((l) => base[l] === 'a')
  return named.at(-1) ?? null
}

export function orcaScenarios(api: Api): ScenarioSet {
  const cases: Case[] = []
  const scenarios: ScenarioSet['scenarios'] = []
  const via = new Map<string, string[]>()
  const note = (row: string, id: string) => via.set(row, [...(via.get(row) ?? []), id])
  const known = new Set(api.endpoints.map((e) => e.id))
  const streaming = new Set(api.endpoints.filter((e) => e.streaming || e.skipped).map((e) => e.id))

  for (const e of api.endpoints) {
    if (streaming.has(e.id) || e.transport === 'http') continue
    const base = params(api.payloads.get(e.id)?.valid)
    const provider = LEVELS.find((l) => PROVIDERS[l].method === e.name)
    const level = levelOf(base)
    if (level === null && provider !== 'repo') continue
    const { steps, reached } = chain(level, api)
    const subject = step(api, reached, e.name, provider === 'repo' ? PROVIDERS.repo.set : {})
    const ns = e.name.split('.')[0]!
    const back = READBACK[ns]
    const readback = back && back !== e.name && known.has(ROW(back)) ? [step(api, reached, back)] : []
    const id = `orca-scn-${e.name}`
    cases.push({ id, row: e.id, op: 'script', input: input(true, [...steps, subject, ...readback]), tags: ['input:scenario', 'state:fresh', 'kind:derived', ...(level ? [`level:${level}`] : [])] })
    scenarios.push({ id, kind: 'derived', rows: [e.id] })
    note(e.id, id)
  }

  for (const story of STORIES) {
    const { steps, reached } = chain(story.from, api)
    const body = story.steps.map((s) => {
      if (!known.has(ROW(s.m))) fail(`story ${story.id} names ${s.m}, which is no endpoint of the crawl`)
      return step(api, reached, s.m, s.set, s.use)
    })
    const id = `orca-story-${story.id}`
    cases.push({ id, row: ROW(story.steps[0]!.m), op: 'script', input: input(true, [...steps, ...body]), tags: ['input:scenario', 'state:fresh', 'kind:story'] })
    const rows = [...new Set([...steps, ...body].map((s) => ROW(s.method)))]
    scenarios.push({ id, kind: 'story', rows, note: story.note })
    for (const r of rows) note(r, id)
  }

  const coverage = coverageOf(api, via, (e) => {
    if (e.streaming) return 'streaming: the script op cannot carry a subscription'
    if (e.skipped) return `not-recorded: ${e.skipped}`
    const base = params(api.payloads.get(e.id)?.valid)
    if (api.payloads.get(e.id)?.valid === undefined) return 'unreadable-schema: only the empty payload is asked'
    const other = Object.keys(base).filter((k) => OTHER_ENTITY.test(k))
    if (other.length) return `unresolved-entity: names ${other.join(', ')}, no provider chain makes it`
    return 'stateless: names no stored entity, the empty-state answer is the answer'
  })
  return { cases, scenarios, coverage }
}
