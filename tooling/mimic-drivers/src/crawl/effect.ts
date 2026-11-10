// T3's contracts are Effect schemas in TypeScript and `effect` is not installed in this repo, so they are read as text: a top-level
// index of `const Name = <expr>;` declarations, a balanced-bracket splitter and a resolver from an expression to a Shape. What the
// resolver does not know (a constructor it has no rule for) becomes `any`, never a guess; `unknown` counts what fell through.
import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import type { Json } from '../protocol.ts'
import type { Shape } from './shape.ts'

const OPENERS: Record<string, string> = { '(': ')', '[': ']', '{': '}' }
const MAX_DEPTH = 12

/** Characters of `text` from `from` up to (not including) the first `stop` at bracket depth zero, strings and comments skipped. */
function scan(text: string, from: number, stop: (c: string, i: number) => boolean): number {
  let depth = 0
  for (let i = from; i < text.length; i++) {
    const c = text[i]!
    if (c === '"' || c === "'" || c === '`') {
      for (i++; i < text.length && text[i] !== c; i++) if (text[i] === '\\') i++
      continue
    }
    if (c === '/' && text[i + 1] === '/') {
      while (i < text.length && text[i] !== '\n') i++
      continue
    }
    if (c === '/' && text[i + 1] === '*') {
      i = text.indexOf('*/', i + 2)
      if (i < 0) return text.length
      i++
      continue
    }
    if (depth === 0 && stop(c, i)) return i
    if (OPENERS[c]) depth++
    else if (c === ')' || c === ']' || c === '}') {
      if (depth === 0) return i
      depth--
    }
  }
  return text.length
}

/** `text` split on commas at depth zero, comments removed, empty pieces dropped. */
export function splitTop(text: string): string[] {
  const out: string[] = []
  let at = 0
  while (at < text.length) {
    const end = scan(text, at, (c) => c === ',')
    out.push(text.slice(at, end))
    at = end + 1
  }
  return out.map((s) => s.replace(/\/\/[^\n]*/g, '').replace(/\/\*[\s\S]*?\*\//g, '').trim()).filter(Boolean)
}

/** The text inside the bracket pair that opens at `open`. */
export function inside(text: string, open: number): string {
  const end = scan(text, open + 1, () => false)
  return text.slice(open + 1, end)
}

export type Index = Map<string, string>

/** Every top-level `const Name = expr;` and `class Name extends Schema.Class<Name>("n")({fields})` of the files, by name. */
export function indexSource(texts: string[]): Index {
  const index: Index = new Map()
  for (const text of texts) {
    for (const m of text.matchAll(/^(?:export )?const ([A-Za-z_]\w*)(?:\s*:[^=\n]+)? = /gm)) {
      const start = m.index! + m[0].length
      const end = scan(text, start, (c) => c === ';')
      if (!index.has(m[1]!)) index.set(m[1]!, text.slice(start, end).trim())
    }
    for (const m of text.matchAll(/^(?:export )?class ([A-Za-z_]\w*) extends Schema\.(?:Class|TaggedClass|TaggedErrorClass)<[^>]*>\([^)]*\)\(\s*\{/gm)) {
      const open = m.index! + m[0].length - 1
      if (!index.has(m[1]!)) index.set(m[1]!, `Schema.Struct({${inside(text, open)}})`)
    }
  }
  return index
}

export function readContracts(dir: string): string[] {
  const walk = (d: string): string[] => readdirSync(d, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(join(d, e.name)) : [join(d, e.name)]))
  return walk(dir).filter((f) => f.endsWith('.ts') && !f.endsWith('.test.ts') && !f.endsWith('.d.ts')).sort().map((f) => readFileSync(f, 'utf8'))
}

const TAIL = /^\.(pipe|check|annotate|annotateKey|brand|makeUnsafe|mapFields|withDecodingDefault|withConstructorDefault|mutable|pick|omit)\(/
const NUMBERS = /^Schema\.(Number|Int|Finite|NonNegativeInt|PositiveInt|Positive|NonNegative|Int32|Uint8)$/

type Resolved = { shape: Shape; opt: boolean }

export class Resolver {
  unknown = new Set<string>()
  private index: Index
  constructor(index: Index) {
    this.index = index
  }

  has(name: string): boolean {
    return this.index.has(name)
  }

  expr(name: string): string | undefined {
    return this.index.get(name)
  }

  /** The object literal `{ key: expr }` of an expression as entries, or null when it is not one. */
  entries(expr: string): [string, string][] | null {
    const t = expr.trim()
    if (!t.startsWith('{')) return null
    return splitTop(inside(t, 0)).flatMap((p) => {
      const m = /^(?:"([^"]+)"|'([^']+)'|([A-Za-z_]\w*))\s*:\s*([\s\S]+)$/.exec(p)
      return m ? [[(m[1] ?? m[2] ?? m[3])!, m[4]!] as [string, string]] : p.startsWith('...') ? [['...', p.slice(3).trim()] as [string, string]] : []
    })
  }

  /** `Name.a.b` followed through the object literals the index holds. */
  member(expr: string, depth: number): string | null {
    const parts = expr.split('.')
    let cur = this.index.get(parts[0]!)
    for (const key of parts.slice(1)) {
      const found = cur ? this.entries(cur)?.find(([k]) => k === key) : undefined
      if (!found) return null
      cur = found[1]
    }
    return cur && depth < MAX_DEPTH ? cur : null
  }

  fields(entries: [string, string][], depth: number): Record<string, { s: Shape; opt: boolean }> {
    const f: Record<string, { s: Shape; opt: boolean }> = {}
    for (const [key, e] of entries) {
      if (key === '...') {
        const spread = this.resolve(e, depth + 1).shape
        if (spread.k === 'obj') Object.assign(f, spread.f)
        continue
      }
      const r = this.resolve(e, depth + 1)
      f[key] = { s: r.shape, opt: r.opt }
    }
    return f
  }

  resolve(raw: string, depth = 0): Resolved {
    const expr = raw.trim()
    const any: Resolved = { shape: { k: 'any' }, opt: false }
    if (depth > MAX_DEPTH) return any
    const tailAt = scan(expr, 0, (_c, i) => TAIL.test(expr.slice(i)) && expr[i] === '.')
    const head = expr.slice(0, tailAt).trim()
    const tail = expr.slice(tailAt)
    const bound = (s: Shape): Shape => {
      if (s.k === 'str') {
        const min = /isMinLength\((\d+)\)/.exec(tail)?.[1]
        const max = /isMaxLength\((\d+)\)/.exec(tail)?.[1]
        return { ...s, ...(min ? { min: Number(min) } : {}), ...(max ? { max: Number(max) } : {}) }
      }
      if (s.k === 'num') {
        const min = /isGreaterThan(?:OrEqualTo)?\((-?\d+)\)/.exec(tail)?.[1]
        const max = /isLessThan(?:OrEqualTo)?\((-?\d+)\)/.exec(tail)?.[1]
        return { ...s, ...(min ? { min: Number(min) } : {}), ...(max ? { max: Number(max) } : {}) }
      }
      return s
    }
    const call = /^([A-Za-z_][\w.]*)\(/.exec(head)
    if (head.startsWith('{')) return { shape: { k: 'obj', f: this.fields(this.entries(head) ?? [], depth) }, opt: false }
    if (call) {
      const name = call[1]!
      const open = call[0].length - 1
      const args = splitTop(inside(head, open))
      switch (name) {
        case 'Schema.Struct':
          return { shape: bound({ k: 'obj', f: this.fields(this.entries(args[0] ?? '{}') ?? [], depth) }), opt: false }
        case 'Schema.optional':
        case 'Schema.optionalKey':
        case 'Schema.UndefinedOr':
        case 'Schema.optionalWith':
          return { shape: this.resolve(args[0] ?? '', depth + 1).shape, opt: true }
        case 'Schema.NullOr':
        case 'Schema.mutable':
        case 'Schema.Option':
        case 'Schema.suspend':
          return { shape: this.resolve((args[0] ?? '').replace(/^\(\)\s*=>\s*/, ''), depth + 1).shape, opt: false }
        case 'Schema.Array':
        case 'Schema.NonEmptyArray':
        case 'Schema.mutableArray': {
          const min = name === 'Schema.NonEmptyArray' ? 1 : Number(/isMinLength\((\d+)\)/.exec(tail)?.[1] ?? 0) || undefined
          return { shape: { k: 'arr', of: this.resolve(args[0] ?? '', depth + 1).shape, ...(min ? { min } : {}) }, opt: false }
        }
        case 'Schema.Union':
        case 'Schema.Union.members': {
          const list = args[0]?.startsWith('[') ? splitTop(inside(args[0], 0)) : args
          return { shape: { k: 'union', of: list.map((a) => this.resolve(a, depth + 1).shape) }, opt: false }
        }
        case 'Schema.Literal':
          return { shape: { k: 'lit', v: [literal(args[0] ?? '')] }, opt: false }
        case 'Schema.Literals':
        case 'Schema.Enum':
          return { shape: { k: 'lit', v: splitTop(inside(args[0] ?? '[]', 0)).map(literal) }, opt: false }
        case 'Schema.Record':
        case 'Schema.ReadonlyMap':
          return { shape: { k: 'rec' }, opt: false }
        case 'Schema.TemplateLiteral':
          return { shape: { k: 'str' }, opt: false }
        default:
          if (this.has(name)) return this.resolve(this.index.get(name)!, depth + 1)
          this.unknown.add(name)
          return any
      }
    }
    if (/^Schema\.(String|NonEmptyString|Trimmed|NonEmptyTrimmedString|Uuid|URL|Date|DateTimeUtc|DateTimeUtcFromString|DateFromString|BigInt|Redacted)$/.test(head)) {
      return { shape: bound({ k: 'str', ...(head.includes('NonEmpty') ? { min: 1 } : {}) }), opt: false }
    }
    if (NUMBERS.test(head)) return { shape: bound({ k: 'num' }), opt: false }
    if (head === 'Schema.Boolean') return { shape: { k: 'bool' }, opt: false }
    if (head === 'Schema.Null') return { shape: { k: 'lit', v: [null] }, opt: false }
    if (/^Schema\.(Unknown|Any|Object|Json|Void|Undefined|Never)$/.test(head)) return any
    if (/^[A-Za-z_]\w*(\.\w+)+$/.test(head) && !head.startsWith('Schema.')) {
      const m = this.member(head, depth)
      if (m) return this.resolve(m, depth + 1)
    }
    if (/^[A-Za-z_]\w*$/.test(head) && this.has(head)) return this.resolve(this.index.get(head)!, depth + 1)
    if (head) this.unknown.add(head.slice(0, 60))
    return any
  }
}

function literal(text: string): Json {
  const t = text.trim()
  if (/^["'`]/.test(t)) return t.slice(1, -1)
  if (t === 'null') return null
  if (t === 'true' || t === 'false') return t === 'true'
  const n = Number(t)
  return Number.isNaN(n) ? t : n
}

/** The variants of a discriminated union: one entry per member object that carries a literal `type`. */
export function variants(r: Resolver, expr: string): { type: string; shape: Shape }[] {
  const s = r.resolve(expr).shape
  if (s.k !== 'union') return []
  const out: { type: string; shape: Shape }[] = []
  for (const m of s.of) {
    const t = m.k === 'obj' ? m.f.type?.s : undefined
    if (t?.k === 'lit' && typeof t.v[0] === 'string') out.push({ type: t.v[0], shape: m })
  }
  return out
}
