// `mimic-crawl-report/1`: what a crawl of one pinned reference found. The Rust side (`wf mimic crawl`) merges it with the committed
// catalog, checks completeness and writes inventory.json, corpus.json and coverage.json beside the set's crawl.toml.
import type { Json } from '../protocol.ts'
import type { InputClass } from './shape.ts'

export type User = 'mobile' | 'cli' | 'internal'
export type Source = 'schema' | 'registry' | 'mobile-calls' | 'cli-source' | 'cli-help' | 'runtime' | 'catalog'

export type Endpoint = {
  id: string
  kind: string
  name: string
  transport: 'rpc' | 'stream' | 'http'
  method?: string
  path?: string
  streaming: boolean
  users: User[]
  sources: Source[]
  /** Who may call it, where the reference says so (Orca's allowlist). */
  access?: string
  /** `schema` the payload was derived from the reference's schema; `golden` a recorded real payload; `empty` the schema is unreadable. */
  input: 'schema' | 'golden' | 'empty'
  classes: Partial<Record<InputClass, number>>
  skipped?: string
}

export type CrawlCase = { id: string; row: string; op: 'script'; input: Json; tags: string[] }
export type CliCommand = { path: string[]; summary?: string; source: Source; file?: string }
export type SourceNote = { name: Source; available: boolean; found: number; note?: string }
/** An endpoint class the crawler cannot see, and the source that would show it. */
export type Blind = { class: string; missing_source: string; detail: string }

export type Report = {
  schema: 'mimic-crawl-report/1'
  app: 'orca' | 't3'
  pin: { version?: string; commit: string }
  sources: SourceNote[]
  endpoints: Endpoint[]
  cli_commands: CliCommand[]
  cases: CrawlCase[]
  blind: Blind[]
}

export const slug = (s: string): string => s.replace(/[^A-Za-z0-9.]+/g, '-').replace(/^-|-$/g, '')
