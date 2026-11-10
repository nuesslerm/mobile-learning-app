// The crawl of the pinned Orca source (references/orca at the commit of tooling/orca-conformance/orca-pin.json): the runtime RPC
// registry (every `defineMethod` / `defineStreamingMethod`), the zod params schema of each, who calls it (the mobile app's call sites,
// the CLI's call sites), the CLI's own command specs, and a request corpus of valid, boundary and invalid inputs per method.
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { repoRoot } from '../lock.ts'
import type { Json } from '../protocol.ts'
import { assertAtPin, orcaRoot } from '../orca-lib/pin.ts'
import { REQUEST_TIMEOUT_MS, registrations, schemasByMethod } from '../corpus/orca.ts'
import { inputs, opaque, type Input } from './shape.ts'
import { fromZod } from './zod.ts'
import { stateTags } from './state.ts'
import { slug, type CliCommand, type CrawlCase, type Endpoint, type Report, type Source, type User } from './types.ts'

const walk = (d: string): string[] => (existsSync(d) ? readdirSync(d, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(join(d, e.name)) : [join(d, e.name)])) : [])
const sources = (dir: string): string[] => walk(dir).filter((f) => /\.tsx?$/.test(f) && !/\.(test|spec)\.tsx?$/.test(f) && !f.includes('__tests__')).map((f) => readFileSync(f, 'utf8'))

type Row = { id: string; kind: string; name: string; phone: boolean; allowlist?: string | null }

function catalogRows(): Row[] {
  const file = join(repoRoot, 'docs/plans/rca-parity/coverage/orca-methods.json')
  return existsSync(file) ? (JSON.parse(readFileSync(file, 'utf8')) as { rows: Row[] }).rows : []
}

/** `path: ['a', 'b']` followed by the next `summary: '...'`, per command spec file under src/cli/specs. */
export function cliSpecs(texts: string[]): CliCommand[] {
  const out: CliCommand[] = []
  for (const text of texts) {
    for (const m of text.matchAll(/^\s+path: \[([^\]]*)\]/gm)) {
      const path = [...m[1]!.matchAll(/'([^']+)'/g)].map((p) => p[1]!)
      const summary = /summary:\s*(?:'((?:[^'\\]|\\.)*)'|"((?:[^"\\]|\\.)*)"|`([^`]*)`)/.exec(text.slice(m.index!, m.index! + 600))
      if (path.length) out.push({ path, source: 'cli-source', ...(summary ? { summary: (summary[1] ?? summary[2] ?? summary[3])!.replace(/\\'/g, "'") } : {}) })
    }
  }
  return out
}

const NOT_ON_THE_WIRE: Record<string, string> = {
  http: 'a third-party URL the phone calls (app store, GitHub releases, DNS): not an Orca host endpoint',
  transport: 'the e2ee handshake is the connection every case opens, answered once per case'
}

export async function orcaCrawl(): Promise<Report> {
  const pin = assertAtPin()
  const root = orcaRoot()
  const found = registrations(root, true)
  const schemas = await schemasByMethod(found)
  const catalog = catalogRows()
  const byName = new Map(catalog.filter((r) => r.kind === 'rpc').map((r) => [r.name, r]))
  const cliTexts = sources(join(root, 'src/cli'))
  const cliBody = cliTexts.join('\n')
  const mobileBody = sources(join(root, 'mobile/src')).join('\n')
  const specs = cliSpecs(walk(join(root, 'src/cli/specs')).filter((f) => f.endsWith('.ts') && !f.endsWith('.test.ts')).map((f) => readFileSync(f, 'utf8')))
  const quoted = (body: string, method: string): boolean => body.includes(`'${method}'`) || body.includes(`"${method}"`)

  const endpoints: Endpoint[] = []
  const cases: CrawlCase[] = []
  for (const reg of [...found].sort((a, b) => a.method.localeCompare(b.method))) {
    const row = byName.get(reg.method)
    const users: User[] = []
    const literal = quoted(mobileBody, reg.method)
    if (row?.phone || literal) users.push('mobile')
    if (quoted(cliBody, reg.method)) users.push('cli')
    if (!users.length) users.push('internal')
    const id = `rpc:${reg.method}`
    const used: Source[] = ['registry', ...(schemas.has(reg.method) ? ['schema' as Source] : []), ...(literal ? ['mobile-calls' as Source] : []), ...(users.includes('cli') ? ['cli-source' as Source] : []), ...(row ? ['catalog' as Source] : [])]
    const endpoint: Endpoint = { id, kind: 'rpc', name: reg.method, transport: reg.streaming ? 'stream' : 'rpc', streaming: reg.streaming, users, sources: used, ...(row?.allowlist ? { access: row.allowlist } : {}), input: 'empty', classes: {} }
    if (reg.streaming) {
      endpoint.skipped = 'streaming method: the script op carries request and response only'
      endpoints.push(endpoint)
      continue
    }
    const schema = schemas.get(reg.method)
    const list: Input[] = schema ? inputs(fromZod(schema as never)) : opaque()
    // a derived valid payload the schema itself refuses is not valid: it is kept as the empty probe, never claimed
    const checked = list.map((i) => (schema && i.class === 'valid' && !(schema as unknown as { safeParse(v: unknown): { success: boolean } }).safeParse(i.payload).success ? { ...i, class: 'empty' as const } : i))
    endpoint.input = schema && checked.some((i) => i.class === 'valid') ? 'schema' : 'empty'
    for (const i of checked) {
      endpoint.classes[i.class] = (endpoint.classes[i.class] ?? 0) + 1
      const params = i.payload as Json | undefined
      cases.push({ id: `orca-${slug(reg.method)}-${i.class}-${slug(i.name)}`, row: id, op: 'script', input: { requests: [{ method: reg.method, ...(params === undefined ? {} : { params }), timeout_ms: REQUEST_TIMEOUT_MS }] } as unknown as Json, tags: [`input:${endpoint.input}`, `class:${i.class}`, ...stateTags(i.class, reg.method)] })
    }
    endpoints.push(endpoint)
  }
  for (const row of catalog.filter((r) => r.kind !== 'rpc').sort((a, b) => a.id.localeCompare(b.id))) {
    endpoints.push({ id: row.id, kind: row.kind, name: row.name, transport: 'http', streaming: false, users: ['mobile'], sources: ['catalog'], input: 'empty', classes: {}, skipped: NOT_ON_THE_WIRE[row.kind] ?? 'not an Orca host endpoint' })
  }
  const mobile = endpoints.filter((e) => e.sources.includes('mobile-calls')).length
  return {
    schema: 'mimic-crawl-report/1',
    app: 'orca',
    pin: { version: pin.version, commit: pin.commit },
    sources: [
      { name: 'registry', available: found.length > 0, found: found.length },
      { name: 'schema', available: schemas.size > 0, found: schemas.size, note: `${found.length - schemas.size} methods have no readable params schema` },
      { name: 'mobile-calls', available: mobileBody.length > 0, found: mobile, note: 'quoted method names under mobile/src, plus the reachability scan of the catalog (wf parity coverage)' },
      { name: 'cli-source', available: cliBody.length > 0, found: endpoints.filter((e) => e.users.includes('cli')).length, note: 'quoted method names under src/cli' },
      { name: 'cli-help', available: specs.length > 0, found: specs.length, note: 'the CLI command specs (src/cli/specs): path and summary of each command `orca --help` prints' },
      { name: 'catalog', available: catalog.length > 0, found: catalog.length },
      { name: 'runtime', available: false, found: 0, note: 'needs a running orcad' }
    ],
    endpoints,
    cli_commands: specs,
    cases,
    blind: [
      { class: 'methods registered only at runtime', missing_source: 'a running orcad (its registry after plugins and dynamic registration)', detail: 'a method whose name is computed, not a literal of a defineMethod block, is not in the source scan' },
      { class: 'mobile call sites with a computed method name', missing_source: 'the type-level call graph of mobile/src (wf parity coverage lists the dynamic sites)', detail: 'sendRequest(method) with a variable: the names come from the callers, which the quoted-name scan sees only where they are literals' },
      { class: 'native module calls', missing_source: 'the Swift and Kotlin sources of mobile/modules', detail: 'a method a native module sends by name is not scanned' }
    ]
  }
}
