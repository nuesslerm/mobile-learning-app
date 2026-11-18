// `scenarios <orca|t3> [<set dir>]`: derive the scenario corpus of an app from its committed crawl and write `corpus.json` (cases of the `script`
// op, recorded by `wf mimic record <set dir>`) and `scenarios.json` (`mimic-scenarios/1`: every endpoint, populated or empty-only with the reason).
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { repoRoot } from '../lock.ts'
import { API_DIR, SET_DIR, loadApi } from './api.ts'
import { orcaScenarios } from './orca.ts'
import { t3AgentCueScenarios, t3AgentMethodScenarios, t3AgentQueryScenarios, t3AgentRealScenarios, t3AgentScenarios } from './t3-agent.ts'
import { t3Scenarios } from './t3.ts'
import type { ScenarioSet, Summary } from './types.ts'

export function scenarios(app: 'orca' | 't3'): ScenarioSet {
  const api = loadApi(app)
  return app === 'orca' ? orcaScenarios(api) : t3Scenarios(api)
}

const lines = (cases: unknown[]) => cases.map((c) => `      ${JSON.stringify(c)}`).join(',\n')

/** The exact bytes of the two files: the test and `--check` compare them with what is committed. */
export function render(app: 'orca' | 't3', set: ScenarioSet, name: string = `${app}-scenarios`): { corpus: string; scenarios: string } {
  const corpus = `{\n  "schema": "mimic-corpus/1",\n  "seed": 0,\n  "generators": [\n    {\n      "kind": "corpus",\n      "name": "${name}",\n      "op": "script",\n      "cases": [\n${lines(set.cases)}\n      ]\n    }\n  ]\n}\n`
  const populated = set.coverage.filter((c) => c.state === 'populated').length
  const doc = { schema: 'mimic-scenarios/1', app, source: API_DIR[app], counts: { endpoints: set.coverage.length, scenarios: set.scenarios.length, populated, emptyOnly: set.coverage.length - populated }, scenarios: set.scenarios, coverage: set.coverage }
  return { corpus, scenarios: `${JSON.stringify(doc, null, 1)}\n` }
}

export function writeScenarios(app: 'orca' | 't3', dir: string = join(repoRoot, SET_DIR[app])): Summary {
  const set = scenarios(app)
  const out = render(app, set)
  mkdirSync(dir, { recursive: true })
  writeFileSync(join(dir, 'corpus.json'), out.corpus)
  writeFileSync(join(dir, 'scenarios.json'), out.scenarios)
  const reasons: Record<string, number> = {}
  for (const c of set.coverage) if (c.reason) reasons[c.reason.split(':')[0]!] = (reasons[c.reason.split(':')[0]!] ?? 0) + 1
  const populated = set.coverage.filter((c) => c.state === 'populated').length
  return { schema: 'mimic-scenarios-summary/1', app, endpoints: set.coverage.length, scenarios: set.scenarios.length, populated, emptyOnly: set.coverage.length - populated, reasons, wrote: [join(dir, 'corpus.json'), join(dir, 'scenarios.json')] }
}

/** The T3 scenarios that need an agent behind the row: their own set, recorded by one hosted `[record t3-v0.1.9-agent-turns]` (the first set stays as recorded). */
export const AGENT_TURNS_DIR = 'clis/t3-serve/cassettes/t3-v0.1.9-agent-turns'

export function agentTurns(): ScenarioSet {
  return t3AgentScenarios(loadApi('t3'))
}

export function writeAgentTurns(dir: string = join(repoRoot, AGENT_TURNS_DIR)): Summary {
  const set = agentTurns()
  const out = render('t3', set, 't3-agent-turns')
  mkdirSync(dir, { recursive: true })
  writeFileSync(join(dir, 'corpus.json'), out.corpus)
  writeFileSync(join(dir, 'scenarios.json'), out.scenarios)
  return { schema: 'mimic-scenarios-summary/1', app: 't3', endpoints: set.coverage.length, scenarios: set.scenarios.length, populated: set.coverage.length, emptyOnly: 0, reasons: {}, wrote: [join(dir, 'corpus.json'), join(dir, 'scenarios.json')] }
}

/** The T3 cases whose row labels the case that runs its agent method (thread.create, project.create and delete, the branch rows, queued-run.start, fork and merge back): its own set, one hosted `[record t3-v0.1.9-agent-methods]`. */
export const AGENT_METHODS_DIR = 'clis/t3-serve/cassettes/t3-v0.1.9-agent-methods'

export function agentMethods(): ScenarioSet {
  return t3AgentMethodScenarios(loadApi('t3'))
}

export function writeAgentMethods(dir: string = join(repoRoot, AGENT_METHODS_DIR)): Summary {
  const set = agentMethods()
  const out = render('t3', set, 't3-agent-methods')
  mkdirSync(dir, { recursive: true })
  writeFileSync(join(dir, 'corpus.json'), out.corpus)
  writeFileSync(join(dir, 'scenarios.json'), out.scenarios)
  return { schema: 'mimic-scenarios-summary/1', app: 't3', endpoints: set.coverage.length, scenarios: set.scenarios.length, populated: set.coverage.length, emptyOnly: 0, reasons: {}, wrote: [join(dir, 'corpus.json'), join(dir, 'scenarios.json')] }
}

/** The T3 query rows asked of a thread the server knows (getThreadTitles, getThreadLinkMetadata, getNativeChildWork, subscribePromptSuggestion): its own set, one hosted `[record t3-v0.1.9-agent-queries]`. */
export const AGENT_QUERIES_DIR = 'clis/t3-serve/cassettes/t3-v0.1.9-agent-queries'

export function agentQueries(): ScenarioSet {
  return t3AgentQueryScenarios(loadApi('t3'))
}

export function writeAgentQueries(dir: string = join(repoRoot, AGENT_QUERIES_DIR)): Summary {
  const set = agentQueries()
  const out = render('t3', set, 't3-agent-queries')
  mkdirSync(dir, { recursive: true })
  writeFileSync(join(dir, 'corpus.json'), out.corpus)
  writeFileSync(join(dir, 'scenarios.json'), out.scenarios)
  return { schema: 'mimic-scenarios-summary/1', app: 't3', endpoints: set.coverage.length, scenarios: set.scenarios.length, populated: set.coverage.length, emptyOnly: 0, reasons: {}, wrote: [join(dir, 'corpus.json'), join(dir, 'scenarios.json')] }
}

/** The T3 rows whose agent method needs a state an answered command made first (unarchive after archive, detach, a subagent stop on a running turn): its own set, one hosted `[record t3-v0.1.9-agent-cues]`. */
export const AGENT_CUES_DIR = 'clis/t3-serve/cassettes/t3-v0.1.9-agent-cues'

export function agentCues(): ScenarioSet {
  return t3AgentCueScenarios(loadApi('t3'))
}

export function writeAgentCues(dir: string = join(repoRoot, AGENT_CUES_DIR)): Summary {
  const set = agentCues()
  const out = render('t3', set, 't3-agent-cues')
  mkdirSync(dir, { recursive: true })
  writeFileSync(join(dir, 'corpus.json'), out.corpus)
  writeFileSync(join(dir, 'scenarios.json'), out.scenarios)
  return { schema: 'mimic-scenarios-summary/1', app: 't3', endpoints: set.coverage.length, scenarios: set.scenarios.length, populated: set.coverage.length, emptyOnly: 0, reasons: {}, wrote: [join(dir, 'corpus.json'), join(dir, 'scenarios.json')] }
}

/** The T3 rows that need a finished or really running turn of a real opencode (checkpoint.rollback, fork then merge back, promote-to-steer): its own set, one hosted `[record t3-v0.1.9-agent-real]`. */
export const AGENT_REAL_DIR = 'clis/t3-serve/cassettes/t3-v0.1.9-agent-real'

export function agentReal(): ScenarioSet {
  return t3AgentRealScenarios(loadApi('t3'))
}

export function writeAgentReal(dir: string = join(repoRoot, AGENT_REAL_DIR)): Summary {
  const set = agentReal()
  const out = render('t3', set, 't3-agent-real')
  mkdirSync(dir, { recursive: true })
  writeFileSync(join(dir, 'corpus.json'), out.corpus)
  writeFileSync(join(dir, 'scenarios.json'), out.scenarios)
  return { schema: 'mimic-scenarios-summary/1', app: 't3', endpoints: set.coverage.length, scenarios: set.scenarios.length, populated: set.coverage.length, emptyOnly: 0, reasons: {}, wrote: [join(dir, 'corpus.json'), join(dir, 'scenarios.json')] }
}
