// `schema` generation spike (mimic plan X6): do the pinned zod schemas of Orca's RPC methods derive payloads the schemas themselves
// accept? Params schemas are read from the registered `defineMethod` blocks of the pinned tree (method name -> schema identifier ->
// the module that exports it), bundled with the pinned zod, walked (zod 4 `_zod.def`) and every derived payload is checked by
// `safeParse` of the same schema. Orca's selectors are `unknown().transform().pipe(string)` wrappers whose JSON Schema is `{}`, so the
// walk follows the `pipe` to its output side instead of going through `z.toJSONSchema`.
import { readdirSync, readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { bundleModule } from '../bundle.ts'
import { repoRoot } from '../lock.ts'
import { assertAtPin, orcaRoot } from '../orca-lib/pin.ts'
import type { Json } from '../protocol.ts'

type Def = { type: string; [k: string]: unknown }
type Schema = { _zod: { def: Def; optin?: string }; safeParse(v: unknown): { success: boolean } }

export type Row = { method: string; schemaName: string | null }

const METHODS_DIR = 'src/main/runtime/rpc/methods'

/** `name: 'ns.x'` followed by the next `params: Ident`, per method file; Ident resolved through the file's own import lines. */
export function registeredParams(namespace: string, root: string = orcaRoot()): { method: string; schemaName: string | null; module: string | null }[] {
  const dir = join(root, METHODS_DIR)
  const out: { method: string; schemaName: string | null; module: string | null }[] = []
  for (const file of readdirSync(dir).filter((f) => f.endsWith('.ts') && !f.endsWith('.test.ts')).sort()) {
    const text = readFileSync(join(dir, file), 'utf8')
    const imports = new Map<string, string>()
    for (const m of text.matchAll(/import\s*\{([^}]*)\}\s*from\s*'([^']+)'/g)) {
      for (const part of m[1]!.split(',')) {
        const name = part.trim().split(/\s+as\s+/).pop()!
        if (name) imports.set(name, resolve(dir, m[2]!))
      }
    }
    for (const m of text.matchAll(new RegExp(`name:\\s*'(${namespace}\\.[A-Za-z0-9]+)'[\\s\\S]*?params:\\s*([A-Za-z0-9_]+)`, 'g'))) {
      out.push({ method: m[1]!, schemaName: m[2]!, module: imports.get(m[2]!) ?? null })
    }
  }
  return out
}

/** The phone's rows of a namespace: what `orca-methods.json` marks as called by the phone. */
export function phoneMethods(namespace: string): string[] {
  const list = JSON.parse(readFileSync(join(repoRoot, 'docs/plans/rca-parity/coverage/orca-methods.json'), 'utf8')) as { rows: { kind: string; name: string; phone: boolean }[] }
  return list.rows.filter((r) => r.kind === 'rpc' && r.phone && r.name.startsWith(`${namespace}.`)).map((r) => r.name).sort()
}

class Underivable extends Error {}

const inner = (d: Def) => d.innerType as Schema
type Check = { check: string; value?: number; minimum?: number; inclusive?: boolean }
const checks = (s: Schema): Check[] => ((s._zod.def.checks as { _zod: { def: Check } }[] | undefined) ?? []).map((c) => c._zod.def)
const lowest = (s: Schema, floor: number): number =>
  checks(s).reduce((lo, c) => (c.check === 'greater_than' ? Math.max(lo, (c.value ?? 0) + (c.inclusive ? 0 : 1)) : c.check === 'min_length' ? Math.max(lo, c.minimum ?? 0) : lo), floor)

/** One value the schema accepts, or Underivable naming the construct the walk does not know. `optional` includes optional keys. */
export function derive(schema: Schema, optional: boolean): unknown {
  const d = schema._zod.def
  switch (d.type) {
    case 'object': {
      const shape = d.shape as Record<string, Schema>
      const out: Record<string, unknown> = {}
      for (const [key, field] of Object.entries(shape)) {
        if (field._zod.optin === 'optional' && !optional) continue
        out[key] = derive(field, optional)
      }
      return out
    }
    case 'pipe': {
      const out = d.out as Schema
      return derive(out._zod.def.type === 'transform' ? (d.in as Schema) : out, optional)
    }
    case 'optional':
    case 'nullable':
    case 'default':
    case 'prefault':
    case 'catch':
    case 'readonly':
    case 'nonoptional':
      return derive(inner(d), optional)
    case 'string': {
      return 'a'.repeat(lowest(schema, 1))
    }
    case 'number': {
      return lowest(schema, 0)
    }
    case 'boolean':
      return true
    case 'enum':
      return Object.values(d.entries as Record<string, unknown>)[0]
    case 'literal':
      return (d.values as unknown[])[0]
    case 'array': {
      return Array.from({ length: lowest(schema, 0) }, () => derive(d.element as Schema, optional))
    }
    case 'union': {
      for (const option of d.options as Schema[]) {
        try {
          const v = derive(option, optional)
          if (schema.safeParse(v).success) return v
        } catch (e) {
          if (!(e instanceof Underivable)) throw e
        }
      }
      throw new Underivable('no option of the union derives a value it accepts')
    }
    case 'record':
      return {}
    case 'unknown':
    case 'any':
      return 'a'
    default:
      throw new Underivable(`zod construct "${d.type}"`)
  }
}

/** Invalid-shape probes of an object payload: no payload, each key dropped, each value replaced by a number. */
export function probes(valid: unknown): { name: string; payload: unknown }[] {
  const list: { name: string; payload: unknown }[] = [{ name: 'no-payload', payload: undefined }, { name: 'empty-object', payload: {} }]
  if (valid && typeof valid === 'object' && !Array.isArray(valid)) {
    for (const key of Object.keys(valid)) {
      const { [key]: _dropped, ...rest } = valid as Record<string, unknown>
      list.push({ name: `drop:${key}`, payload: rest }, { name: `number:${key}`, payload: { ...(valid as object), [key]: 7 } })
    }
  }
  return list
}

export type RowReport = {
  method: string
  schema: string | null
  derived: boolean
  valid: boolean
  reason?: string
  payload?: Json
  full_valid?: boolean
  probes?: { total: number; rejected: number; accepted: string[] }
}

export type Report = { schema: 'mimic-schema-spike/1'; namespace: string; pin: string; rows: RowReport[]; counts: { rows: number; derived: number; valid: number; probes_total: number; probes_rejected: number } }

let loaded: Promise<Map<string, Schema>> | undefined

function schemasOf(found: ReturnType<typeof registeredParams>): Promise<Map<string, Schema>> {
  loaded ??= (async () => {
    const wanted = found.filter((f) => f.module)
    const imports = wanted.map((f, i) => `import { ${f.schemaName} as s${i} } from '${f.module}'`).join('\n')
    const table = wanted.map((f, i) => `  ${JSON.stringify(f.method)}: s${i}`).join(',\n')
    const mod = await bundleModule<{ schemas: Record<string, Schema> }>(`${imports}\nexport const schemas = {\n${table}\n}\n`)
    return new Map(Object.entries(mod.schemas))
  })()
  return loaded
}

export async function spike(namespace: string): Promise<Report> {
  const pin = assertAtPin()
  const found = registeredParams(namespace)
  const schemas = await schemasOf(found)
  const rows: RowReport[] = []
  for (const method of phoneMethods(namespace)) {
    const entry = found.find((f) => f.method === method)
    const schema = schemas.get(method)
    if (!entry || !schema) {
      rows.push({ method, schema: entry?.schemaName ?? null, derived: false, valid: false, reason: entry ? `${entry.schemaName} is not imported from a module` : 'no registered params' })
      continue
    }
    try {
      const payload = derive(schema, false)
      const full = derive(schema, true)
      const all = probes(payload)
      const rejected = all.filter((p) => !schema.safeParse(p.payload).success)
      rows.push({
        method, schema: entry.schemaName, derived: true, valid: schema.safeParse(payload).success, payload: payload as Json, full_valid: schema.safeParse(full).success,
        probes: { total: all.length, rejected: rejected.length, accepted: all.filter((p) => !rejected.includes(p)).map((p) => p.name) }
      })
    } catch (e) {
      if (!(e instanceof Underivable)) throw e
      rows.push({ method, schema: entry.schemaName, derived: false, valid: false, reason: e.message })
    }
  }
  const counts = {
    rows: rows.length,
    derived: rows.filter((r) => r.derived).length,
    valid: rows.filter((r) => r.valid).length,
    probes_total: rows.reduce((n, r) => n + (r.probes?.total ?? 0), 0),
    probes_rejected: rows.reduce((n, r) => n + (r.probes?.rejected ?? 0), 0)
  }
  return { schema: 'mimic-schema-spike/1', namespace, pin: pin.commit, rows, counts }
}


/** The derived payloads as corpus cases of the `script` op (one request each), the rows' ids as their parity rows. */
export function cases(report: Report): { id: string; row: string; op: 'script'; input: Json; tags: string[] }[] {
  return report.rows
    .filter((r) => r.valid)
    .map((r) => ({ id: `schema-${r.method}`, row: `rpc:${r.method}`, op: 'script' as const, input: { requests: [{ method: r.method, params: r.payload as Json }] }, tags: ['schema'] }))
}
