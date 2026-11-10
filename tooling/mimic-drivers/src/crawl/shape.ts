// One request-input model for both references (zod for Orca, Effect schema text for T3) and the generator of the three input classes
// of a method: valid (the smallest payload the schema accepts), boundary (everything optional filled, a limit of a bounded leaf) and
// invalid (no payload, a required key dropped, a key of the wrong type). Pure: no I/O, no randomness.
import type { Json } from '../protocol.ts'

export type Shape =
  | { k: 'str'; min?: number; max?: number }
  | { k: 'num'; int?: boolean; min?: number; max?: number }
  | { k: 'bool' }
  | { k: 'lit'; v: Json[] }
  | { k: 'arr'; of: Shape; min?: number }
  | { k: 'obj'; f: Record<string, { s: Shape; opt: boolean }> }
  | { k: 'union'; of: Shape[] }
  | { k: 'rec' }
  | { k: 'any' }

/** `empty`: the schema could not be read, so only the empty payload is asked and the server's own decode answer is what gets recorded. */
export type InputClass = 'valid' | 'boundary' | 'invalid' | 'empty'
/** `payload` undefined means the request is sent without params. */
export type Input = { class: InputClass; name: string; payload: Json | undefined }

const STRING_CAP = 256

/** The smallest value the shape accepts (`full` also fills optional keys and gives arrays an element). */
export function value(s: Shape, full: boolean): Json {
  switch (s.k) {
    case 'str':
      return 'a'.repeat(Math.max(s.min ?? 0, 1))
    case 'num':
      return Math.max(s.min ?? 0, 0)
    case 'bool':
      return true
    case 'lit':
      return s.v[0] ?? null
    case 'arr':
      return Array.from({ length: Math.max(s.min ?? 0, full ? 1 : 0) }, () => value(s.of, full))
    case 'obj': {
      const out: Record<string, Json> = {}
      for (const [key, f] of Object.entries(s.f)) if (full || !f.opt) out[key] = value(f.s, full)
      return out
    }
    case 'union':
      return s.of.length > 0 ? value(s.of[0]!, full) : null
    case 'rec':
      return {}
    case 'any':
      return 'a'
  }
}

/** The limit of the first bounded leaf of an object payload: a string at its maximum length, a number at its maximum or minimum. */
function edge(s: Shape): { key: string; value: Json } | null {
  if (s.k !== 'obj') return null
  for (const [key, f] of Object.entries(s.f)) {
    if (f.s.k === 'str' && f.s.max !== undefined) return { key, value: 'a'.repeat(Math.min(f.s.max, STRING_CAP)) }
    if (f.s.k === 'num' && (f.s.max !== undefined || f.s.min !== undefined)) return { key, value: (f.s.max ?? f.s.min) as number }
  }
  return null
}

/** A value of another type than the field's, so the server's decoder has to refuse it. */
const wrongType = (s: Shape): Json => (s.k === 'str' || s.k === 'lit' ? 7 : 'wrong')

export function inputs(s: Shape): Input[] {
  const valid = value(s, false)
  const out: Input[] = [{ class: 'valid', name: 'min', payload: valid }]
  const full = value(s, true)
  if (JSON.stringify(full) !== JSON.stringify(valid)) out.push({ class: 'boundary', name: 'full', payload: full })
  const e = edge(s)
  if (e) out.push({ class: 'boundary', name: `limit:${e.key}`, payload: { ...(valid as object), [e.key]: e.value } })
  out.push({ class: 'invalid', name: 'no-payload', payload: undefined })
  if (s.k === 'obj') {
    // a discriminator literal is dropped last: the server must still reach the variant to refuse the rest
    const fields = Object.entries(s.f)
    const first = fields.find(([, f]) => !f.opt && f.s.k !== 'lit') ?? fields.find(([, f]) => !f.opt) ?? fields[0]
    if (first) {
      const [key, f] = first
      if (!f.opt) {
        const { [key]: _dropped, ...rest } = valid as Record<string, Json>
        out.push({ class: 'invalid', name: `drop:${key}`, payload: rest })
      }
      out.push({ class: 'invalid', name: `wrong-type:${key}`, payload: { ...(valid as object), [key]: wrongType(f.s) } })
    }
  } else {
    out.push({ class: 'invalid', name: 'wrong-type', payload: wrongType(s) })
  }
  return out
}

/** The same inputs for a method whose schema the walk cannot read: only the empty and the absent payload, named so by class. */
export function opaque(): Input[] {
  return [
    { class: 'empty', name: 'empty-object', payload: {} },
    { class: 'invalid', name: 'no-payload', payload: undefined }
  ]
}
