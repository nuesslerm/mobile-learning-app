// The crawl of the pinned T3 source (references/t3code at the commit of tooling/t3-conformance/t3-pin.json): every wire endpoint the
// contracts package defines (Effect RPC tags, the HttpApi routes, the variants of the two discriminated commands), the CLI's commands,
// who uses each, and a request corpus of valid, boundary and invalid inputs derived from each payload schema.
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { repoRoot } from '../lock.ts'
import type { Json } from '../protocol.ts'
import { SKIPPED, THREAD, bearer, form, goldenPayloads, request, type Entry } from '../corpus/t3.ts'
import { Resolver, indexSource, inside, readContracts, splitTop, variants } from './effect.ts'
import { inputs, opaque, type Input, type Shape } from './shape.ts'
import { stateTags } from './state.ts'
import { slug, type CliCommand, type CrawlCase, type Endpoint, type Report, type Source, type User } from './types.ts'

export function t3Root(): string {
  return process.env.MIMIC_T3_ROOT || join(repoRoot, 'references', 't3code')
}

export function t3Pin(): { version: string; commit: string } {
  const pin = JSON.parse(readFileSync(join(repoRoot, 'tooling/t3-conformance/t3-pin.json'), 'utf8'))
  return { version: pin.version, commit: pin.commit }
}

const walk = (d: string): string[] => (existsSync(d) ? readdirSync(d, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(join(d, e.name)) : [join(d, e.name)])) : [])
const sources = (dir: string): string[] => walk(dir).filter((f) => f.endsWith('.ts') && !f.endsWith('.test.ts')).map((f) => readFileSync(f, 'utf8'))

const SETTLE_MS = 400

type Wire = { id: string; kind: string; name: string; transport: Endpoint['transport']; method?: string; path?: string; via?: string; shape: Shape | null; golden?: Json }

/** `NAME_METHODS = { key: "tag" }` maps of the contracts, by `NAME.key`. */
function methodMaps(r: Resolver, texts: string[]): Map<string, string> {
  const out = new Map<string, string>()
  for (const text of texts) {
    for (const m of text.matchAll(/^(?:export )?const ([A-Z][A-Z0-9_]*_METHODS) = \{/gm)) {
      const body = inside(text, m.index! + m[0].length - 1)
      for (const e of r.entries(`{${body}}`) ?? []) out.set(`${m[1]}.${e[0]}`, e[1].replace(/^["']|["']$/g, ''))
    }
  }
  return out
}

/** Every `Rpc.make(<tag>, { payload, stream })`: the tag, the payload expression and whether the reply is a stream. */
function rpcs(texts: string[], names: Map<string, string>): { tag: string; payload: string | null; stream: boolean }[] {
  const out: { tag: string; payload: string | null; stream: boolean }[] = []
  for (const text of texts) {
    for (const m of text.matchAll(/Rpc\.make\(/g)) {
      const args = splitTop(inside(text, m.index! + m[0].length - 1))
      const first = args[0] ?? ''
      const tag = /^["'](.+)["']$/.test(first) ? first.slice(1, -1) : names.get(first)
      if (!tag) continue
      const opts = new Resolver(new Map()).entries(args[1] ?? '{}') ?? []
      out.push({ tag, payload: opts.find(([k]) => k === 'payload')?.[1] ?? null, stream: opts.some(([k, v]) => k === 'stream' && v.trim() === 'true') })
    }
  }
  return out
}

/**
 * The routes of the groups that `HttpApi.make("environment")` mounts (the API the T3 server itself serves; the relay's `client` API is
 * another server's): `class G extends HttpApiGroup.make("g").add(HttpApiEndpoint.post("name", "/path", { payload }))`.
 */
function routes(texts: string[], r: Resolver): { group: string; name: string; method: string; path: string; payload: string | null }[] {
  const out: { group: string; name: string; method: string; path: string; payload: string | null }[] = []
  for (const text of texts) {
    const root = /class \w+ extends HttpApi\.make\("environment"\)([\s\S]*?)\{\}/.exec(text)
    if (!root) continue
    const mounted = new Set([...root[1]!.matchAll(/\.add\((\w+)\)/g)].map((m) => m[1]!))
    const classes = [...text.matchAll(/^class (\w+) extends HttpApiGroup\.make\("([^"]+)"/gm)].map((m) => ({ at: m.index!, cls: m[1]!, group: m[2]! }))
    classes.forEach((c, i) => {
      if (!mounted.has(c.cls)) return
      const body = text.slice(c.at, classes[i + 1]?.at ?? text.length)
      for (const m of body.matchAll(/HttpApiEndpoint\.(get|post|put|patch|delete)\(/g)) {
        const args = splitTop(inside(body, m.index! + m[0].length - 1))
        const [name, path] = [args[0]?.replace(/^"|"$/g, ''), args[1]]
        const literal = path?.startsWith('"') ? path.slice(1, -1) : (r.expr(path ?? '') ?? '').replace(/^"|"$/g, '')
        let opts = r.entries(args[2] ?? '{}')
        if (!opts && /^\w+$/.test(args[2] ?? '')) opts = r.entries(r.expr(args[2]!) ?? '{}')
        if (name && literal) out.push({ group: c.group, name, method: m[1]!.toUpperCase(), path: literal, payload: opts?.find(([k]) => k === 'payload')?.[1] ?? null })
      }
    })
  }
  return out
}

function commandsOf(cliTexts: string[]): CliCommand[] {
  const seen = new Set<string>()
  const out: CliCommand[] = []
  for (const text of cliTexts) {
    for (const m of text.matchAll(/Command\.make\(\s*"([a-z][a-z0-9-]*)"/g)) {
      if (!seen.has(m[1]!)) {
        seen.add(m[1]!)
        out.push({ path: [m[1]!], source: 'cli-source' })
      }
    }
  }
  return out
}

/** Subcommand names in the text of `t3 <cmd> --help`: the lines that follow a COMMANDS or SUBCOMMANDS heading. */
export function helpCommands(text: string): string[] {
  const out: string[] = []
  let on = false
  for (const line of text.split('\n')) {
    if (/^\s*(sub)?commands:?\s*$/i.test(line)) {
      on = true
      continue
    }
    if (on && /^\S/.test(line) && !/^\s/.test(line)) on = false
    const m = on ? /^\s+([a-z][a-z0-9-]*)(?:\s|$)/.exec(line) : null
    if (m) out.push(m[1]!)
  }
  return out
}

export type Runtime = { dir?: string }

function userList(ids: { mobile: boolean; cli: boolean }): User[] {
  const u: User[] = []
  if (ids.mobile) u.push('mobile')
  if (ids.cli) u.push('cli')
  return u.length ? u : ['internal']
}

type CatalogRow = { id: string; phone: boolean }

export function t3Crawl(options: { runtime?: string } = {}): Report {
  const root = t3Root()
  const contractsDir = join(root, 'packages/contracts/src')
  if (!existsSync(contractsDir)) throw new Error(`no T3 source at ${contractsDir}: initialise the references/t3code submodule or set MIMIC_T3_ROOT`)
  const texts = readContracts(contractsDir)
  const r = new Resolver(indexSource(texts))
  const names = methodMaps(r, texts)
  const wire: Wire[] = []
  const golden = goldenPayloads()

  for (const rpc of rpcs(texts, names)) {
    if (wire.some((w) => w.id === `rpc:${rpc.tag}`)) continue
    const shape = rpc.payload ? r.resolve(rpc.payload).shape : ({ k: 'obj', f: {} } as Shape)
    wire.push({ id: `rpc:${rpc.tag}`, kind: 'rpc', name: rpc.tag, transport: rpc.stream ? 'stream' : 'rpc', shape, golden: golden.byTag.get(rpc.tag) })
    const kind = rpc.tag.endsWith('.dispatchCommand') ? 'command' : rpc.tag.endsWith('.mutate') ? 'mutation' : null
    if (kind && rpc.payload) {
        for (const v of variants(r, rpc.payload)) wire.push({ id: `${kind}:${v.type}`, kind, name: v.type, transport: 'rpc', shape: v.shape, golden: golden.byType.get(v.type), via: rpc.tag })
    }
  }
  for (const route of routes(texts, r)) {
    wire.push({ id: `http:${route.group}.${route.name}`, kind: 'http', name: `${route.group}.${route.name}`, transport: 'http', method: route.method, path: route.path, shape: route.payload ? r.resolve(route.payload).shape : null })
  }

  const cliTexts = [...sources(join(root, 'apps/server/src/cli')), ...walk(join(root, 'apps/server/src')).filter((f) => /\/(bin|binCli|cli[A-Za-z]*)\.ts$/.test(f)).map((f) => readFileSync(f, 'utf8'))]
  const cliBody = cliTexts.join('\n')
  const mobileTexts = sources(join(root, 'apps/mobile/src'))
  const mobileBody = mobileTexts.join('\n')
  const catalog = catalogRows()
  const cliCommands = commandsOf(cliTexts)
  const notes: Report['sources'] = [
    { name: 'schema', available: true, found: wire.length, note: `${r.unknown.size} schema constructors read as any` },
    { name: 'cli-source', available: cliTexts.length > 0, found: cliCommands.length },
    { name: 'mobile-calls', available: mobileTexts.length > 0, found: 0, note: 'literal tags and WS_METHODS keys under apps/mobile/src, plus the reachability scan of the catalog (wf parity coverage)' },
    { name: 'catalog', available: catalog.size > 0, found: catalog.size }
  ]
  const blind: Report['blind'] = [
    { class: 'handlers registered without a schema', missing_source: 'a running t3 server (RPC introspection is not exposed)', detail: 'a method the server implements that no contract declares cannot be seen from the source' },
    { class: 'runtime self-description', missing_source: 'a running t3 server', detail: '/.well-known/t3/environment answers only on a live server: capture it on the VM with `wf mimic crawl t3 --runtime <dir>`' }
  ]
  if (options.runtime) {
    const files = existsSync(options.runtime) ? readdirSync(options.runtime).filter((f) => /^help.*\.txt$/.test(f)) : []
    const fromHelp = files.flatMap((f) => helpCommands(readFileSync(join(options.runtime!, f), 'utf8')))
    for (const name of fromHelp) if (!cliCommands.some((c) => c.path[0] === name)) cliCommands.push({ path: [name], source: 'cli-help' })
    notes.push({ name: 'cli-help', available: files.length > 0, found: new Set(fromHelp).size, note: options.runtime })
    notes.push({ name: 'runtime', available: existsSync(join(options.runtime, 'descriptor.json')), found: 0 })
    blind.splice(0, blind.length, blind[0]!)
  } else {
    notes.push({ name: 'cli-help', available: false, found: 0, note: 'needs the release binary: `t3 --help` of every subcommand, captured on the VM' })
    notes.push({ name: 'runtime', available: false, found: 0, note: 'needs a running server' })
  }

  const endpoints: Endpoint[] = []
  const cases: CrawlCase[] = []
  for (const w of wire.sort((a, b) => a.id.localeCompare(b.id))) {
    const tagLiteral = new RegExp(`["'\`]${w.name.replace(/\./g, '\\.')}["'\`]`)
    const keyOf = [...names].filter(([, v]) => v === w.name).map(([k]) => k)
    const mentions = (body: string) => tagLiteral.test(body) || keyOf.some((k) => body.includes(k)) || (w.kind === 'http' && body.includes(`"${w.path}"`))
    const row = catalog.get(w.id)
    const literal = mentions(mobileBody)
    const users = userList({ mobile: Boolean(row?.phone) || literal, cli: mentions(cliBody) })
    const seen: Source[] = ['schema', ...(row ? ['catalog' as Source] : []), ...(literal ? ['mobile-calls' as Source] : []), ...(users.includes('cli') ? ['cli-source' as Source] : [])]
    const endpoint: Endpoint = { id: w.id, kind: w.kind, name: w.name, transport: w.transport, ...(w.method && w.kind === 'http' ? { method: w.method, path: w.path } : {}), streaming: w.transport === 'stream', users, sources: seen, input: 'empty', classes: {} }
    const skip = SKIPPED[w.name]
    if (skip) {
      endpoint.skipped = skip
      endpoints.push(endpoint)
      continue
    }
    const list: Input[] = w.shape ? inputs(w.shape) : opaque()
    const picked: Input[] = w.golden !== undefined ? [{ class: 'valid', name: 'golden', payload: w.golden }, ...list.filter((i) => i.class !== 'valid')] : list
    endpoint.input = w.golden !== undefined ? 'golden' : w.shape && w.shape.k !== 'any' ? 'schema' : 'empty'
    for (const i of picked) {
      endpoint.classes[i.class] = (endpoint.classes[i.class] ?? 0) + 1
      cases.push({ id: `t3-${slug(w.id)}-${i.class}-${slug(i.name)}`, row: w.id, op: 'script', input: script(w, i) as unknown as Json, tags: [`input:${endpoint.input}`, `class:${i.class}`, ...stateTags(i.class, w.name, w.kind === 'http' ? w.method : undefined, /^command:|revoke|subscribe/i.test(w.id))] })
    }
    endpoints.push(endpoint)
  }
  return { schema: 'mimic-crawl-report/1', app: 't3', pin: t3Pin(), sources: notes.map((n) => (n.name === 'mobile-calls' ? { ...n, found: endpoints.filter((e) => e.sources.includes('mobile-calls')).length } : n)), endpoints, cli_commands: cliCommands, cases, blind }
}

function catalogRows(): Map<string, CatalogRow> {
  const file = join(repoRoot, 'docs/plans/rca-parity/coverage/t3-methods.json')
  if (!existsSync(file)) return new Map()
  const table = JSON.parse(readFileSync(file, 'utf8')) as { rows: CatalogRow[] }
  return new Map(table.rows.map((r) => [r.id, r]))
}

/** The session script of one input: pair, open the socket with a project and a thread already made, ask once, close. */
function script(w: Wire, input: Input): { script: Entry[]; settle_ms: number; prelude?: 'session' } {
  const payload = input.payload === undefined ? {} : input.payload
  if (w.kind === 'http') {
    if (w.name === 'auth.token') return { script: [form('{credential}')], settle_ms: SETTLE_MS }
    if (w.name === 'metadata.descriptor') return { script: [{ http: { method: 'GET', path: w.path!, headers: { accept: '*/*' }, body: null } }], settle_ms: SETTLE_MS }
    const path = w.path!.replace(/:threadId/g, THREAD).replace(/:use/g, 'rpc').replace(/:[A-Za-z]+/g, 'x-1')
    const body = w.method === 'GET' || w.method === 'DELETE' ? null : input.payload === undefined ? '' : input.payload
    const headers = typeof body === 'object' && body !== null ? { ...bearer, 'content-type': 'application/json' } : bearer
    return { prelude: 'session', script: [{ http: { method: w.method!, path, headers, body } }, { 'ws-close': {} }], settle_ms: SETTLE_MS }
  }
  const tag = w.via ?? w.name
  const body = w.kind === 'command' || w.kind === 'mutation' ? { ...(payload as object), type: w.name } : payload
  return { prelude: 'session', script: [request('1', tag, body as Json), { 'ws-close': {} }], settle_ms: SETTLE_MS }
}
