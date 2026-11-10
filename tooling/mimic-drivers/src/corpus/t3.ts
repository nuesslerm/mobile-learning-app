// Corpus of the T3 recording (mimic plan R1): one `script` case per phone row of t3-methods.json. The coverage table says WHICH wire rows
// the phone uses (rpc tag, command type, mutation type, http method and path); request payloads come from the client side of the
// imported goldens where a golden carries one for that tag or type, and are otherwise the empty payload (the server's own decode answer is
// what gets recorded). Answers are never written here: the pinned server really runs under `wf mimic record`.
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { repoRoot } from '../lock.ts'
import type { Json } from '../protocol.ts'
import type { Case, Disposition } from './types.ts'

const GOLDEN = 'clis/t3-serve/cassettes/t3-v0.1.6/interactions.jsonl'
const TABLE = 'docs/plans/rca-parity/coverage/t3-methods.json'
export const THREAD = 'conformance-thread'
export const PROJECT = 'conformance-project'

type Row = { id: string; kind: string; name: string; phone: boolean; method?: string; path?: string }
export type Entry = Record<string, Json>

export const SKIPPED: Record<string, string> = {
  'server.updateServer': 'replaces the installed server binary',
  'server.updateServerWithProgress': 'replaces the installed server binary',
  'server.commitDesktopUpdate': 'commits a desktop app update; there is no desktop'
}

/** The client's request frames in the imported goldens: the payload for an rpc tag, and for a command or mutation by its `type`. */
export function goldenPayloads(): { byTag: Map<string, Json>; byType: Map<string, Json> } {
  const byTag = new Map<string, Json>()
  const byType = new Map<string, Json>()
  for (const line of readFileSync(join(repoRoot, GOLDEN), 'utf8').split('\n').filter(Boolean)) {
    const interaction = JSON.parse(line) as { steps: { dir: string; payload: { _tag?: string; tag?: string; payload?: Json } }[] }
    for (const s of interaction.steps) {
      if (s.dir !== 'c2s' || s.payload?._tag !== 'Request' || !s.payload.tag) continue
      const payload = JSON.parse(JSON.stringify(s.payload.payload ?? {}).replaceAll('<workspace>', '{workspace}')) as Json
      const type = (payload as { type?: unknown }).type
      if (typeof type === 'string' && !byType.has(type) && Object.keys(payload as object).length > 1) byType.set(type, payload)
      if (!byTag.has(s.payload.tag) && Object.keys(payload as object).length > 0 && typeof type !== 'string') byTag.set(s.payload.tag, payload)
    }
  }
  return { byTag, byType }
}

export const form = (credential: string) => ({
  http: {
    method: 'POST', path: '/oauth/token', headers: { accept: '*/*', 'content-type': 'application/x-www-form-urlencoded' },
    body: {
      grant_type: 'urn:ietf:params:oauth:grant-type:token-exchange', subject_token_type: 'urn:t3:params:oauth:token-type:environment-bootstrap',
      requested_token_type: 'urn:ietf:params:oauth:token-type:access_token', subject_token: credential, client_label: 't3-conformance', client_os: 'linux', client_device_type: 'bot'
    }
  }
})

export const bearer = { accept: '*/*', authorization: 'Bearer {access_token}' }
export const request = (id: string, tag: string, payload: Json): Entry => ({ 'ws-send': { _tag: 'Request', id, tag, payload, headers: [] } })

/** A session with a project and a thread already made, so a row that names one has something to find. */
export function prelude(golden: ReturnType<typeof goldenPayloads>): Entry[] {
  return [
    form('{credential}'),
    { http: { method: 'POST', path: '/api/auth/websocket-ticket/rpc', headers: bearer, body: null } },
    { 'ws-open': { path: '/ws?orchestrationProtocol=2&wsTicket={ticket}' } },
    { ...request('p1', 'projects.mutate', golden.byType.get('project.create') ?? {}), await: 'exit' },
    { ...request('p2', 'orchestration.dispatchCommand', golden.byType.get('thread.create') ?? {}), await: 'exit' }
  ]
}

const commandPayload = (type: string, golden: ReturnType<typeof goldenPayloads>): Json =>
  golden.byType.get(type) ?? { type, commandId: `conformance-cmd-${type}`, threadId: THREAD }

const mutationPayload = (type: string, golden: ReturnType<typeof goldenPayloads>): Json =>
  golden.byType.get(type) ?? { type, commandId: `conformance-cmd-${type}`, projectId: PROJECT }

export type T3Corpus = { cases: Case[]; coverage: Disposition[] }

export function t3Corpus(): T3Corpus {
  const golden = goldenPayloads()
  const table = JSON.parse(readFileSync(join(repoRoot, TABLE), 'utf8')) as { rows: Row[] }
  const cases: Case[] = []
  const coverage: Disposition[] = []
  const add = (row: Row, id: string, script: Entry[], input: string, settle_ms = 400) => {
    cases.push({ id, row: row.id, op: 'script', input: { script, settle_ms } as unknown as Json, tags: [`input:${input}`] })
    coverage.push({ row: row.id, status: 'case', case: id, input })
  }
  for (const row of table.rows.filter((r) => r.phone).sort((a, b) => a.id.localeCompare(b.id))) {
    const id = `t3-${row.kind}-${row.name}`
    if (SKIPPED[row.name]) {
      coverage.push({ row: row.id, status: 'skipped', reason: SKIPPED[row.name]! })
      continue
    }
    if (row.kind === 'http') {
      if (row.name === 'auth.token') {
        add(row, id, [form('{credential}')], 'golden')
        continue
      }
      if (row.name === 'metadata.descriptor') {
        add(row, id, [{ http: { method: 'GET', path: row.path!, headers: { accept: '*/*' }, body: null } }], 'golden')
        continue
      }
      const path = row.path!.replace(':threadId', THREAD).replace(':use', 'rpc')
      add(row, id, [...prelude(golden), { http: { method: row.method!, path, headers: bearer, body: row.method === 'POST' ? '' : null } }, { 'ws-close': {} }], 'empty')
      continue
    }
    let tag = row.name
    let payload: Json = golden.byTag.get(row.name) ?? {}
    let input = golden.byTag.has(row.name) ? 'golden' : 'empty'
    if (row.kind === 'command') {
      tag = 'orchestration.dispatchCommand'
      payload = commandPayload(row.name, golden)
      input = golden.byType.has(row.name) ? 'golden' : 'derived-keys'
    } else if (row.kind === 'mutation') {
      tag = 'projects.mutate'
      payload = mutationPayload(row.name, golden)
      input = golden.byType.has(row.name) ? 'golden' : 'derived-keys'
    }
    add(row, id, [...prelude(golden), request('1', tag, payload), { 'ws-close': {} }], input)
  }
  return { cases, coverage }
}
