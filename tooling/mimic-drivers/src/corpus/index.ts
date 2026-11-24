// `corpus <app> <set dir>`: the request inputs of a recording set, derived from the coverage table and the pinned references.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { orcaCorpus } from './orca.ts'
import { t3Corpus } from './t3.ts'
import type { Case, Disposition } from './types.ts'

export type Summary = { schema: 'mimic-corpus-summary/1'; app: string; enumerated: number; cases: number; skipped: number; inputs: Record<string, number>; wrote: string[] }

export async function writeCorpus(app: 'orca' | 't3', dir: string): Promise<Summary> {
  const { cases, coverage } = app === 'orca' ? await orcaCorpus() : t3Corpus()
  return write(app, dir, cases, coverage)
}

function write(app: string, dir: string, cases: Case[], coverage: Disposition[], withCoverage = true): Summary {
  mkdirSync(dir, { recursive: true })
  const corpus = { schema: 'mimic-corpus/1', seed: 0, generators: [{ kind: 'corpus', name: `${app}-rows`, op: 'script', cases }] }
  const table = { schema: 'mimic-coverage/1', app, rows: coverage }
  writeFileSync(join(dir, 'corpus.json'), `${JSON.stringify(corpus, null, 2)}\n`)
  if (withCoverage) writeFileSync(join(dir, 'coverage.json'), `${JSON.stringify(table, null, 2)}\n`)
  const inputs: Record<string, number> = {}
  for (const c of coverage) if (c.input) inputs[c.input] = (inputs[c.input] ?? 0) + 1
  return { schema: 'mimic-corpus-summary/1', app, enumerated: coverage.length, cases: cases.length, skipped: coverage.filter((c) => c.status === 'skipped').length, inputs, wrote: withCoverage ? [join(dir, 'corpus.json'), join(dir, 'coverage.json')] : [join(dir, 'corpus.json')] }
}

/**
 * The cases of each shard after a pin move: a case the table still derives stays in its shard (as derived now), one it no longer
 * derives is dropped, and a new one goes to the shard with the fewest cases (the first on a tie), in id order, so the same inputs give
 * the same shards on every machine.
 */
export function assignCases(shards: Case[][], derived: Case[]): Case[][] {
  const byId = new Map(derived.map((c) => [c.id, c]))
  const out = shards.map((s) => s.flatMap((c) => byId.get(c.id) ?? []))
  const placed = new Set(out.flat().map((c) => c.id))
  for (const c of derived.filter((d) => !placed.has(d.id)).sort((a, b) => a.id.localeCompare(b.id))) {
    out.reduce((least, s) => (s.length < least.length ? s : least)).push(c)
  }
  return out
}

/** `refresh-corpus <app> <dir...>`: re-derive the corpus of the shards of an app, keeping the case each shard already holds. */
export async function refreshCorpus(app: 'orca' | 't3', dirs: string[]): Promise<Summary[]> {
  const { cases, coverage } = app === 'orca' ? await orcaCorpus() : t3Corpus()
  const read = (dir: string) => (JSON.parse(readFileSync(join(dir, 'corpus.json'), 'utf8')) as { generators: { cases: Case[] }[] }).generators.flatMap((g) => g.cases)
  const shards = assignCases(dirs.map(read), cases)
  // a shard keeps a coverage table only when it already had one (orca-v1.4.259-recorded does, the T3 shards do not)
  return dirs.map((dir, i) => write(app, dir, shards[i]!, coverage, existsSync(join(dir, 'coverage.json'))))
}
