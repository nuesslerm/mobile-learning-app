// Given the phone coverage table of an app, When the recording corpus is derived from it, Then every phone row is accounted for exactly once
// (a case, or skipped with its reason), every case names a row of the table, and the corpus committed beside the sets is what the generator makes.
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { test } from 'node:test'
import { repoRoot } from '../src/lock.ts'
import { t3Corpus } from '../src/corpus/t3.ts'
import { orcaCorpus } from '../src/corpus/orca.ts'
import { assignCases } from '../src/corpus/index.ts'
import type { Case, Disposition } from '../src/corpus/types.ts'

type Row = { id: string; phone: boolean }

function phoneRows(app: string): string[] {
  const table = JSON.parse(readFileSync(join(repoRoot, `docs/plans/rca-parity/coverage/${app}-methods.json`), 'utf8')) as { rows: Row[] }
  return table.rows.filter((r) => r.phone).map((r) => r.id)
}

function accounted(rows: string[], cases: Case[], coverage: Disposition[]): void {
  const seen = coverage.map((d) => d.row)
  assert.deepEqual([...new Set(seen)].sort(), [...new Set(rows)].sort(), 'the coverage table names every phone row of the methods list, once each')
  assert.equal(seen.length, new Set(seen).size, 'no row is accounted for twice')
  assert.equal(new Set(cases.map((c) => c.id)).size, cases.length, 'case ids are unique')
  for (const d of coverage) assert.ok(d.status === 'case' ? cases.some((c) => c.id === d.case && c.row === d.row) : d.reason, `${d.row}: a case it names, or the reason it is skipped`)
  for (const c of cases) assert.ok(rows.includes(c.row), `${c.id} names a row of the table`)
}

function committed(dirs: string[]): Case[] {
  return dirs.flatMap((d) => (JSON.parse(readFileSync(join(repoRoot, d, 'corpus.json'), 'utf8')) as { generators: { cases: Case[] }[] }).generators.flatMap((g) => g.cases))
}

test('Given the T3 methods list, When the corpus is derived, Then each phone row is a case or skipped with a reason, and the committed shards hold exactly those cases', () => {
  const { cases, coverage } = t3Corpus()
  accounted(phoneRows('t3'), cases, coverage)
  const shards = ['1', '2', '3'].map((i) => `clis/t3-serve/cassettes/t3-recorded-${i}`)
  assert.deepEqual(committed(shards).map((c) => c.id).sort(), cases.map((c) => c.id).sort())
})

test('Given the Orca methods list and the checkout at the pin, When the corpus is derived, Then each phone row is a case or skipped with a reason', { skip: !process.env.MIMIC_ORCA_ROOT && 'needs MIMIC_ORCA_ROOT: a checkout of references/orca at the pin' }, async () => {
  const { cases, coverage } = await orcaCorpus()
  accounted(phoneRows('orca'), cases, coverage)
  assert.deepEqual(committed(['clis/orca-serve/cassettes/orca-recorded']).map((c) => c.id).sort(), cases.map((c) => c.id).sort())
})

test('Given shards from before a pin move, When the cases are assigned again, Then kept cases stay put, gone ones leave and new ones go to the smallest shard in id order', () => {
  const c = (id: string, v = 0): Case => ({ id, row: id, op: 'script', input: { v }, tags: [] })
  const shards = [[c('a'), c('b'), c('gone')], [c('d')], [c('e'), c('f')]]
  const derived = [c('a', 1), c('b'), c('d'), c('e'), c('f'), c('z-new'), c('y-new')]
  const out = assignCases(shards, derived).map((s) => s.map((x) => x.id))
  assert.deepEqual(out, [['a', 'b', 'z-new'], ['d', 'y-new'], ['e', 'f']])
  assert.deepEqual(assignCases(shards, derived)[0]![0]!.input, { v: 1 }, 'a kept case is the one derived now')
})

test('Given the committed shards and the table they came from, When the cases are assigned again, Then every shard keeps its cases', () => {
  const shards = ['1', '2', '3'].map((i) => committed([`clis/t3-serve/cassettes/t3-recorded-${i}`]))
  const { cases } = t3Corpus()
  const ids = new Set(cases.map((x) => x.id))
  const out = assignCases(shards, cases)
  for (const [i, s] of shards.entries()) assert.deepEqual(out[i]!.filter((x) => s.some((o) => o.id === x.id)).map((x) => x.id), s.filter((o) => ids.has(o.id)).map((o) => o.id))
})
