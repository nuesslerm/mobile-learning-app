// Values one request's answer teaches the next one of a scenario: `capture` names JSON pointers into the answer, `{var}` in a later request
// is replaced by what they found. Shared by the Orca and T3 drivers; a variable no earlier answer produced is reported, never sent as text.
import type { Json } from './protocol.ts'

/** A value a later request names as `{var}`: the first of `from` (JSON pointers into the answer) that holds a string or number. */
export type Capture = Record<string, { from: string[]; prefix?: string }>

export type Vars = Record<string, Json>

const placeholder = /\{([A-Za-z][\w.-]*)\}/g

/** A string that is exactly `{var}` becomes the value itself (a number stays one); inside a longer string the text is substituted. */
export function fill(v: Json, vars: Vars): Json {
  if (typeof v === 'string') {
    const whole = /^\{([A-Za-z][\w.-]*)\}$/.exec(v)
    if (whole && whole[1]! in vars) return vars[whole[1]!]!
    return v.replace(placeholder, (m, name: string) => (name in vars ? String(vars[name]) : m))
  }
  if (Array.isArray(v)) return v.map((x) => fill(x, vars))
  if (v !== null && typeof v === 'object') return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, fill(x, vars)]))
  return v
}

export function unbound(v: Json, vars: Vars, names: Set<string>): string | undefined {
  const text = JSON.stringify(v)
  for (const m of text.matchAll(placeholder)) if (names.has(m[1]!) && !(m[1]! in vars)) return m[1]
  return undefined
}

export function pointer(doc: Json, ptr: string): Json | undefined {
  let at: Json | undefined = doc
  for (const part of ptr.split('/').slice(1)) {
    if (at === null || typeof at !== 'object') return undefined
    at = (at as Record<string, Json>)[part.replaceAll('~1', '/').replaceAll('~0', '~')]
  }
  return at
}

export function captured(response: Json, capture: Capture): Vars {
  const out: Vars = {}
  for (const [name, { from, prefix }] of Object.entries(capture)) {
    for (const ptr of from) {
      const v = pointer(response, ptr)
      if (typeof v === 'string' || typeof v === 'number') {
        out[name] = typeof v === 'string' ? `${prefix ?? ''}${v}` : v
        break
      }
    }
  }
  return out
}

