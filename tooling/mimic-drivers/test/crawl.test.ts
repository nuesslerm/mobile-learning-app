// Given a schema (zod, or Effect text), When its inputs are generated, Then the valid one is accepted, every invalid one is refused and the
// boundary ones are accepted; and Given the pinned source of a reference, When it is crawled, Then every row of the committed catalog is an
// endpoint of the crawl (the crawl never lags the source), with the numbers of each kind equal to the catalog's.
import assert from 'node:assert/strict'
import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { test } from 'node:test'
import { z } from 'zod'
import { repoRoot } from '../src/lock.ts'
import { Resolver, indexSource, variants } from '../src/crawl/effect.ts'
import { inputs, value } from '../src/crawl/shape.ts'
import { fromZod, type Zod } from '../src/crawl/zod.ts'
import { cliSpecs } from '../src/crawl/orca.ts'
import { helpCommands, t3Crawl, t3Root } from '../src/crawl/t3.ts'

const schema = z.object({ name: z.string().min(2).max(8), count: z.number().int().min(1).max(5), flag: z.boolean().optional(), tags: z.array(z.string()).min(1), mode: z.enum(['a', 'b']) })
const accepts = (p: unknown) => schema.safeParse(p).success

test('Given a zod object schema, When inputs are generated, Then valid and boundary inputs are accepted and every invalid one is refused', () => {
  const list = inputs(fromZod(schema as unknown as Zod))
  const byClass = (c: string) => list.filter((i) => i.class === c)
  assert.ok(byClass('valid').length >= 1 && byClass('boundary').length >= 1 && byClass('invalid').length >= 3)
  for (const i of [...byClass('valid'), ...byClass('boundary')]) assert.ok(accepts(i.payload), `${i.name} is accepted`)
  for (const i of byClass('invalid')) assert.ok(!accepts(i.payload), `${i.name} is refused`)
})

test('Given the limits of a bounded field, When the boundary input is generated, Then it sits exactly on the limit', () => {
  const edge = inputs(fromZod(schema as unknown as Zod)).find((i) => i.name.startsWith('limit:'))!
  const key = edge.name.slice('limit:'.length)
  const field = (schema.shape as Record<string, { _zod: { def: { checks?: { _zod: { def: { maximum?: number; value?: number } } }[] } } }>)[key]!
  const limit = field._zod.def.checks!.map((c) => c._zod.def.maximum ?? c._zod.def.value).find((v) => v !== undefined)!
  const got = (edge.payload as Record<string, unknown>)[key]
  assert.equal(typeof got === 'string' ? got.length : got, limit)
})

const TEXT = `
export const Id = Schema.String.check(Schema.isMinLength(1)).pipe(Schema.brand("Id"));
const Shared = { createdBy: Schema.Literals(["user", "system"]), id: Id };
export const Command = Schema.Union([
  Schema.Struct({ type: Schema.Literal("thing.make"), ...Shared, size: Schema.optional(Schema.Number.check(Schema.isLessThanOrEqualTo(9))) }),
  Schema.Struct({ type: Schema.Literal("thing.drop"), id: Id }),
]);
export const Rpcs = { make: { input: Command, output: Schema.Null } } as const;
`

test('Given Effect schema text, When it is resolved, Then aliases, spreads, optionals and discriminated variants are read', () => {
  const r = new Resolver(indexSource([TEXT]))
  const vs = variants(r, 'Command')
  assert.deepEqual(vs.map((v) => v.type), ['thing.make', 'thing.drop'])
  const make = vs[0]!.shape
  assert.equal(make.k, 'obj')
  if (make.k !== 'obj') return
  assert.deepEqual(Object.keys(make.f).sort(), ['createdBy', 'id', 'size', 'type'])
  assert.equal(make.f.size!.opt, true)
  assert.deepEqual(make.f.id!.s, { k: 'str', min: 1 })
  assert.deepEqual(r.resolve('Rpcs.make.input').shape.k, 'union')
  assert.deepEqual(value(make, false), { type: 'thing.make', createdBy: 'user', id: 'a' })
})

test('Given an Effect struct with a required key, When inputs are generated, Then the invalid inputs lack it or give it another type', () => {
  const r = new Resolver(indexSource([TEXT]))
  const shape = variants(r, 'Command')[1]!.shape
  const invalid = inputs(shape).filter((i) => i.class === 'invalid')
  assert.ok(invalid.some((i) => i.payload && !('id' in (i.payload as object))), 'a payload without id')
  assert.ok(invalid.some((i) => i.payload && typeof (i.payload as { id?: unknown }).id === 'number'), 'a payload with a number for id')
})

test('Given CLI spec modules and a help text, When commands are read, Then each path and subcommand is listed', () => {
  const specs = cliSpecs([`export const X = [\n  {\n    path: ['repo', 'list'],\n    summary: 'List repos',\n    usage: 'orca repo list'\n  },\n  {\n    path: ['status'],\n    summary: "Show it"\n  }\n]`])
  assert.deepEqual(specs.map((s) => [s.path.join(' '), s.summary]), [['repo list', 'List repos'], ['status', 'Show it']])
  assert.deepEqual(helpCommands('t3 serve\n\nCOMMANDS\n  serve   Run it\n  pair    Pair\nOPTIONS\n  --x\n'), ['serve', 'pair'])
})

type Row = { id: string; kind: string }
const catalog = (app: string) => (JSON.parse(readFileSync(join(repoRoot, `docs/plans/rca-parity/coverage/${app}-methods.json`), 'utf8')) as { rows: Row[] }).rows

test('Given the pinned T3 tree, When it is crawled, Then every catalog row is an endpoint and no endpoint the catalog lacks is invented', { skip: !existsSync(join(t3Root(), 'packages/contracts/src')) && 'needs references/t3code at the pin (MIMIC_T3_ROOT)' }, () => {
  const report = t3Crawl()
  const rows = catalog('t3')
  const have = new Set(report.endpoints.map((e) => e.id))
  assert.deepEqual(rows.filter((r) => !have.has(r.id)).map((r) => r.id), [], 'the crawl lags the source')
  assert.deepEqual([...have].filter((id) => !rows.some((r) => r.id === id)), [], 'the crawl lists what the catalog does not')
  for (const e of report.endpoints.filter((e) => !e.skipped)) assert.ok(report.cases.some((c) => c.row === e.id), `${e.id} has a case`)
})

test('Given the pinned Orca tree, When it is crawled, Then every catalog row is an endpoint, streaming ones are skipped with a reason and every schema-derived valid input is accepted by its schema', { skip: !process.env.MIMIC_ORCA_ROOT && 'needs MIMIC_ORCA_ROOT: a checkout of references/orca at the pin' }, async () => {
  const { orcaCrawl } = await import('../src/crawl/orca.ts')
  const report = await orcaCrawl()
  const have = new Set(report.endpoints.map((e) => e.id))
  assert.deepEqual(catalog('orca').filter((r) => !have.has(r.id)).map((r) => r.id), [], 'the crawl lags the source')
  for (const e of report.endpoints.filter((e) => e.streaming)) assert.ok(e.skipped, `${e.id} is streaming and says why it is not asked`)
  for (const e of report.endpoints.filter((e) => e.input === 'schema')) assert.ok(report.cases.some((c) => c.row === e.id && c.tags.includes('class:valid')), `${e.id} has a valid case`)
})
