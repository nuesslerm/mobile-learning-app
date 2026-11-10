// Orca's params schemas (zod 4, bundled from the pinned tree by `bundleModule`) as a Shape. Selectors are `unknown().transform().pipe(string)`
// wrappers, so a pipe is followed to its input side when its output is a transform, as the X6 walk does.
import type { Shape } from './shape.ts'

type Def = { type: string; [k: string]: unknown }
export type Zod = { _zod: { def: Def; optin?: string }; safeParse(v: unknown): { success: boolean } }
type Check = { check: string; value?: number; minimum?: number; maximum?: number; inclusive?: boolean }

const checks = (s: Zod): Check[] => ((s._zod.def.checks as { _zod: { def: Check } }[] | undefined) ?? []).map((c) => c._zod.def)
const MAX_DEPTH = 8

export function fromZod(schema: Zod, depth = 0): Shape {
  const d = schema._zod.def
  if (depth > MAX_DEPTH) return { k: 'any' }
  const cs = checks(schema)
  switch (d.type) {
    case 'object': {
      const f: Record<string, { s: Shape; opt: boolean }> = {}
      for (const [key, field] of Object.entries(d.shape as Record<string, Zod>)) f[key] = { s: fromZod(field, depth + 1), opt: field._zod.optin === 'optional' }
      return { k: 'obj', f }
    }
    case 'pipe': {
      const out = d.out as Zod
      return fromZod(out._zod.def.type === 'transform' ? (d.in as Zod) : out, depth + 1)
    }
    case 'optional':
    case 'nullable':
    case 'default':
    case 'prefault':
    case 'catch':
    case 'readonly':
    case 'nonoptional':
      return fromZod(d.innerType as Zod, depth + 1)
    case 'string': {
      const min = cs.filter((c) => c.check === 'min_length').reduce<number | undefined>((m, c) => Math.max(m ?? 0, c.minimum ?? 0), undefined)
      const max = cs.filter((c) => c.check === 'max_length').reduce<number | undefined>((m, c) => Math.min(m ?? Infinity, c.maximum ?? Infinity), undefined)
      return { k: 'str', ...(min !== undefined ? { min } : {}), ...(max !== undefined ? { max } : {}) }
    }
    case 'number': {
      const lo = cs.filter((c) => c.check === 'greater_than').map((c) => (c.value ?? 0) + (c.inclusive ? 0 : 1))
      const hi = cs.filter((c) => c.check === 'less_than').map((c) => (c.value ?? 0) - (c.inclusive ? 0 : 1))
      return { k: 'num', ...(lo.length ? { min: Math.max(...lo) } : {}), ...(hi.length ? { max: Math.min(...hi) } : {}) }
    }
    case 'boolean':
      return { k: 'bool' }
    case 'enum':
      return { k: 'lit', v: Object.values(d.entries as Record<string, string | number>) }
    case 'literal':
      return { k: 'lit', v: d.values as (string | number | boolean | null)[] }
    case 'array': {
      const min = cs.find((c) => c.check === 'min_length')?.minimum
      return { k: 'arr', of: fromZod(d.element as Zod, depth + 1), ...(min !== undefined ? { min } : {}) }
    }
    case 'union':
      return { k: 'union', of: (d.options as Zod[]).map((o) => fromZod(o, depth + 1)) }
    case 'record':
      return { k: 'rec' }
    default:
      return { k: 'any' }
  }
}
