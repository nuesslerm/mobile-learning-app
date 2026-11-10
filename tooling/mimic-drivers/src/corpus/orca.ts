// Corpus of the Orca recording (mimic plan R1): one `script` case per phone row of orca-methods.json. The pinned tree is read only for the
// request INPUTS: which method registers which params schema, and a payload the schema accepts (the X6 walk). The answers are never
// written here: they come from an orcad that really ran (`wf mimic record`).
import { readdirSync, readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { bundleModule } from '../bundle.ts'
import { repoRoot } from '../lock.ts'
import { assertAtPin, orcaRoot } from '../orca-lib/pin.ts'
import type { Json } from '../protocol.ts'
import { derive } from '../orca/schema.ts'
import type { Case, Disposition } from './types.ts'

const METHODS_DIR = 'src/main/runtime/rpc/methods'
export const REQUEST_TIMEOUT_MS = 8000

/** `const NAME = 'value'` and `const NAME = { key: 'value' }` (as `NAME.key`) across the pinned src tree: method names given by a constant. */
function stringConstants(root: string): Map<string, string> {
  const out = new Map<string, string>()
  const walk = (d: string): string[] => readdirSync(d, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(join(d, e.name)) : [join(d, e.name)]))
  for (const file of walk(join(root, 'src')).filter((f) => /\.ts$/.test(f) && !/\.(test|d)\.ts$/.test(f))) {
    const text = readFileSync(file, 'utf8')
    for (const m of text.matchAll(/(?:^|\n)\s*(?:export )?const ([A-Z][A-Z0-9_]*)\s*=\s*'([A-Za-z0-9.]+)'/g)) out.set(m[1]!, m[2]!)
    for (const m of text.matchAll(/(?:^|\n)\s*(?:export )?const ([A-Za-z_]\w*)\s*=\s*\{([^}]*)\}/g)) {
      for (const e of m[2]!.matchAll(/([A-Za-z_]\w*)\s*:\s*'([A-Za-z0-9.]+)'/g)) out.set(`${m[1]}.${e[1]}`, e[2]!)
    }
  }
  return out
}

export type Registration = { method: string; streaming: boolean; schemaName: string | null; module: string | null }

/** Every `defineMethod` / `defineStreamingMethod` of the pinned methods dir: name, kind, and the params identifier with the module it is imported from. */
/** `byConstant` also resolves a `name: SOME_CONSTANT` the way the catalog scan does; the R1 corpus (superseded by the crawl) leaves those out. */
export function registrations(root: string = orcaRoot(), byConstant = false): Registration[] {
  const dir = join(root, METHODS_DIR)
  const out: Registration[] = []
  const walk = (d: string): string[] => readdirSync(d, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(join(d, e.name)) : [join(d, e.name)]))
  const constants = byConstant ? stringConstants(root) : new Map<string, string>()
  for (const file of walk(dir).filter((f) => f.endsWith('.ts') && !f.endsWith('.test.ts')).sort()) {
    const text = readFileSync(file, 'utf8')
    const imports = new Map<string, string>()
    for (const m of text.matchAll(/import\s*\{([^}]*)\}\s*from\s*'([^']+)'/g)) {
      for (const part of m[1]!.split(',')) {
        const name = part.trim().split(/\s+as\s+/).pop()!
        if (name) imports.set(name, resolve(file, '..', m[2]!))
      }
    }
    const starts = [...text.matchAll(/define(Streaming)?Method\s*\(?\s*\{?/g)].map((m) => ({ at: m.index!, streaming: m[1] !== undefined }))
    starts.forEach((s, i) => {
      const block = text.slice(s.at, starts[i + 1]?.at ?? text.length)
      const named = /name:\s*(?:'([A-Za-z0-9.]+)'|([A-Za-z_][A-Za-z0-9_.]*))/.exec(block)
      const name = named?.[1] ?? (named?.[2] ? constants.get(named[2]) : undefined)
      if (!name) return
      const ident = /params:\s*([A-Za-z0-9_]+)\s*[,\n]/.exec(block)?.[1]
      out.push({ method: name, streaming: s.streaming, schemaName: ident ?? null, module: ident ? (imports.get(ident) ?? null) : null })
    })
  }
  return out
}

/** Phone rows of the coverage table: `rpc` kind, called by the phone. */
export function phoneRows(): { name: string; id: string }[] {
  const table = JSON.parse(readFileSync(join(repoRoot, 'docs/plans/rca-parity/coverage/orca-methods.json'), 'utf8')) as { rows: { id: string; kind: string; name: string; phone: boolean }[] }
  return table.rows.filter((r) => r.phone && r.kind === 'rpc').map((r) => ({ id: r.id, name: r.name })).sort((a, b) => a.name.localeCompare(b.name))
}

export type Schema = Parameters<typeof derive>[0]

export async function schemasByMethod(found: Registration[]): Promise<Map<string, Schema>> {
  const byModule = new Map<string, Registration[]>()
  for (const f of found.filter((x) => x.module && x.schemaName)) byModule.set(f.module!, [...(byModule.get(f.module!) ?? []), f])
  const out = new Map<string, Schema>()
  for (const [module, list] of byModule) {
    const names = [...new Set(list.map((l) => l.schemaName!))]
    try {
      const mod = await bundleModule<Record<string, Schema>>(`import * as m from '${module}'\nexport const ${names.map((n) => `${n} = (m as any).${n}`).join(', ')}\n`)
      for (const l of list) if (mod[l.schemaName!]) out.set(l.method, mod[l.schemaName!]!)
    } catch {
      // a module that cannot be bundled leaves its methods on the empty-payload input
    }
  }
  return out
}

export type OrcaCorpus = { cases: Case[]; coverage: Disposition[]; pin: string }

export async function orcaCorpus(): Promise<OrcaCorpus> {
  const pin = assertAtPin()
  const found = registrations()
  const schemas = await schemasByMethod(found)
  const cases: Case[] = []
  const coverage: Disposition[] = []
  for (const row of phoneRows()) {
    const reg = found.find((f) => f.method === row.name)
    const rowKey = `rpc:${row.name}`
    if (!reg) {
      coverage.push({ row: rowKey, status: 'skipped', reason: 'not registered in the pinned methods dir (dynamic registration)' })
      continue
    }
    if (reg.streaming) {
      coverage.push({ row: rowKey, status: 'skipped', reason: 'streaming method: the script op carries request and response only' })
      continue
    }
    const schema = schemas.get(row.name)
    let params: Json = {}
    let input = 'empty'
    if (schema) {
      try {
        params = derive(schema, false) as Json
        input = schema.safeParse(params).success ? 'schema' : 'empty'
        if (input === 'empty') params = {}
      } catch {
        params = {}
      }
    }
    cases.push({ id: `orca-${row.name}`, row: rowKey, op: 'script', input: { requests: [{ method: row.name, params, timeout_ms: REQUEST_TIMEOUT_MS }] }, tags: [`input:${input}`] })
    coverage.push({ row: rowKey, status: 'case', case: `orca-${row.name}`, input })
  }
  for (const [what, why] of [
    ['http', 'a third-party URL the phone calls (app store, GitHub releases, DNS): not an Orca host endpoint'],
    ['transport', 'the e2ee handshake is the connection every case opens, answered once per case']
  ] as const) {
    const table = JSON.parse(readFileSync(join(repoRoot, 'docs/plans/rca-parity/coverage/orca-methods.json'), 'utf8')) as { rows: { id: string; kind: string; phone: boolean }[] }
    for (const r of table.rows.filter((x) => x.phone && x.kind === what)) coverage.push({ row: r.id, status: 'skipped', reason: why })
  }
  return { cases, coverage, pin: pin.commit }
}
