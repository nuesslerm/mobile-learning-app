// `corpus <app> <set dir>`: the request inputs of a recording set, derived from the coverage table and the pinned references.
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { orcaCorpus } from './orca.ts'
import { t3Corpus } from './t3.ts'
import type { Case, Disposition } from './types.ts'

export type Summary = { schema: 'mimic-corpus-summary/1'; app: string; enumerated: number; cases: number; skipped: number; inputs: Record<string, number>; wrote: string[] }

export async function writeCorpus(app: 'orca' | 't3', dir: string): Promise<Summary> {
  const { cases, coverage } = app === 'orca' ? await orcaCorpus() : t3Corpus()
  return write(app, dir, cases, coverage)
}

function write(app: string, dir: string, cases: Case[], coverage: Disposition[]): Summary {
  mkdirSync(dir, { recursive: true })
  const corpus = { schema: 'mimic-corpus/1', seed: 0, generators: [{ kind: 'corpus', name: `${app}-rows`, op: 'script', cases }] }
  const table = { schema: 'mimic-coverage/1', app, rows: coverage }
  writeFileSync(join(dir, 'corpus.json'), `${JSON.stringify(corpus, null, 2)}\n`)
  writeFileSync(join(dir, 'coverage.json'), `${JSON.stringify(table, null, 2)}\n`)
  const inputs: Record<string, number> = {}
  for (const c of coverage) if (c.input) inputs[c.input] = (inputs[c.input] ?? 0) + 1
  return { schema: 'mimic-corpus-summary/1', app, enumerated: coverage.length, cases: cases.length, skipped: coverage.filter((c) => c.status === 'skipped').length, inputs, wrote: [join(dir, 'corpus.json'), join(dir, 'coverage.json')] }
}
