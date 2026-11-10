// `scenarios <orca|t3> [<set dir>]`: derive the scenario corpus of an app from its committed crawl and write `corpus.json` (cases of the `script`
// op, recorded by `wf mimic record <set dir>`) and `scenarios.json` (`mimic-scenarios/1`: every endpoint, populated or empty-only with the reason).
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { repoRoot } from '../lock.ts'
import { API_DIR, SET_DIR, loadApi } from './api.ts'
import { orcaScenarios } from './orca.ts'
import { t3Scenarios } from './t3.ts'
import type { ScenarioSet, Summary } from './types.ts'

export function scenarios(app: 'orca' | 't3'): ScenarioSet {
  const api = loadApi(app)
  return app === 'orca' ? orcaScenarios(api) : t3Scenarios(api)
}

const lines = (cases: unknown[]) => cases.map((c) => `      ${JSON.stringify(c)}`).join(',\n')

/** The exact bytes of the two files: the test and `--check` compare them with what is committed. */
export function render(app: 'orca' | 't3', set: ScenarioSet): { corpus: string; scenarios: string } {
  const corpus = `{\n  "schema": "mimic-corpus/1",\n  "seed": 0,\n  "generators": [\n    {\n      "kind": "corpus",\n      "name": "${app}-scenarios",\n      "op": "script",\n      "cases": [\n${lines(set.cases)}\n      ]\n    }\n  ]\n}\n`
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
