// Reads the committed crawl of an app (inventory.json, corpus.json beside crawl.toml): no reference checkout is needed, so the scenarios are derived
// offline and the same on every machine.
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { repoRoot } from '../lock.ts'
import type { Json } from '../protocol.ts'
import type { Api, Endpoint, RowPayloads } from './types.ts'

export const API_DIR = { orca: 'clis/orca-serve/cassettes/orca-api', t3: 'clis/t3-serve/cassettes/t3-api' } as const
export const SET_DIR = { orca: 'clis/orca-serve/cassettes/orca-scenarios', t3: 'clis/t3-serve/cassettes/t3-scenarios' } as const

type CrawlCase = { id: string; row: string; input: Json; tags: string[] }

/** The payload one corpus case sends: Orca's first request, T3's last websocket request frame. */
function payloadOf(app: 'orca' | 't3', input: Json): { payload?: Json; frame?: Json } {
  if (app === 'orca') return { payload: (input as { requests: { params?: Json }[] }).requests[0]?.params ?? {} }
  const frames = ((input as { script: Record<string, Json>[] }).script).flatMap((e) => ('ws-send' in e ? [e['ws-send'] as { payload?: Json }] : []))
  return { payload: frames.at(-1)?.payload, frame: frames.at(-1) as Json }
}

export function loadApi(app: 'orca' | 't3'): Api {
  const dir = join(repoRoot, API_DIR[app])
  const inventory = JSON.parse(readFileSync(join(dir, 'inventory.json'), 'utf8')) as { endpoints: Endpoint[] }
  const corpus = JSON.parse(readFileSync(join(dir, 'corpus.json'), 'utf8')) as { generators: { cases: CrawlCase[] }[] }
  const payloads = new Map<string, RowPayloads>()
  for (const c of corpus.generators.flatMap((g) => g.cases)) {
    const cls = c.tags.find((t) => t.startsWith('class:'))?.slice(6)
    if (cls !== 'valid' && cls !== 'boundary' && cls !== 'empty') continue
    const { payload, frame } = payloadOf(app, c.input)
    if (payload === undefined) continue
    const row = payloads.get(c.row) ?? {}
    row[cls] ??= payload
    if (cls !== 'boundary') row.frame ??= frame
    payloads.set(c.row, row)
  }
  return { app, endpoints: inventory.endpoints, payloads }
}
